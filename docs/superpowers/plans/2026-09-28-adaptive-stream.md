# Adaptive Stream Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Each task is a separate delegated review unit. The orchestrator coordinates dependencies, reviews each deliverable, and performs permitted verification. Preserve unrelated working-tree changes.

**Goal:** Adapt a live H.264 desktop stream to sustained browser-network and host-pipeline pressure while keeping the peer session active, and expose adaptive/fixed mode and effective settings to the user.

**Architecture:** A host controller chooses a tier bounded by the browser's configured ceiling, using network and host pressure as separate inputs. The browser and host exchange compact versioned messages over a WebRTC data channel; a Tokio watch channel carries only the latest encoder policy into capture. The existing peer connection, media track, bounded encoded-frame queue, and HTTP offer/answer flow stay in place.

**Tech Stack:** Rust 2024, Tokio, serde/serde_json, `webrtc` 0.21.0 data channels, `openh264` 0.9.8 software encoding, and the embedded HTML/CSS/JavaScript client.

**Spec:** [../specs/2026-09-28-adaptive-stream-design.md](../specs/2026-09-28-adaptive-stream-design.md)

## Global Constraints

- Adaptive mode is selected by default while fixed mode preserves the selected exact settings.
- Keep the existing peer connection and video session alive during adaptation; do not renegotiate or replace the PeerConnection for a tier change.
- The raw one-frame capture channel remains the stale-frame boundary; the encoded H.264 queue remains bounded and must not drop arbitrary delta frames.
- Missing browser or host signals remain unknown; absence is never treated as healthy conditions or zero values.
- Reject malformed, stale, and out-of-order telemetry; keep updates compact and bounded so newer browser summaries supersede stale unsent summaries.
- If the data channel cannot be established, keep video running; host-only adaptation may continue while the UI reports missing browser feedback.
- If adaptation or encoder reinitialization fails, retain last known working settings where possible, report the error, and keep the peer session alive.
- Keep network and host pressure classification separate. RTT, encode duration, and browser statistics alone are not end-to-end latency.
- Do not claim a fixed latency or that the stream feels local on every network or device. Quest glass-to-glass performance remains unverified.
- Do not add dependencies, hardware encoding, a new codec, multiple display streams, or a new signaling service.
- Centralize pressure durations, recovery conditions, tier mapping, and rate limits. Mark initial numeric values provisional until sustained measurements support tuning.

## Review Focus

- Stats omit or reset counters: missing fields remain unavailable and do not imply health; check manually with browser stats unavailable.
- Browser telemetry is stale, duplicated, malformed, or reordered: ignore it without changing controller state; manually inject invalid messages and sequence/timestamp cases.
- A transient loss/jitter or host-load burst clears before persistence expires: no tier change; manually run a pressure burst shorter than the configured interval.
- Network and host pressure occur independently and together: classify each source separately and use the lower required tier; manually induce network impairment and host load separately, then together.
- Data channel, runtime apply, encoder recreation, or keyframe recovery fails: retain the active session and last working policy and display degraded/error state; manually close the channel and exercise an available runtime failure path.

## Shared File Map

- `src/adaptive.rs` (new): versioned control/status types, telemetry validation, tier ladder, pressure classification, hysteresis, and controller diagnostics.
- `src/quality.rs`: existing `Preset`/`StreamSettings`, offer parsing, output dimensions, and ceiling-bounded tier candidates.
- `src/main.rs`: crate module registration for the new `adaptive` module.
- `src/capture.rs`: host timing metrics, runtime latest-value settings receiver, cadence gating, encoder reconfiguration, and bounded frame delivery.
- `src/server.rs`: optional offer mode parsing with backward-compatible adaptive default.
- `src/rtc.rs`: host-created per-session controller, accepted data channel, message routing, policy updates, metrics feedback, and status sends.
- `web/index.html`: browser-created data channel, one-second getStats reporting, mode/ceiling controls, and effective/degraded status UI.
- `docs/architecture.md`, `docs/performance.md`, `docs/implementation-status.md`: flow, diagnostics, evidence, and remaining validation boundaries.

## Shared Interface Contract

Task 1 owns these declarations. Later tasks must use their exact serialized field names and enum tags.
The following `Preset` and `StreamSettings` shapes show the existing `src/quality.rs` types whose serde derives are extended; they are not new adaptive-module duplicates.

```rust
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdaptationMode { Adaptive, Fixed }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ControlMessage {
    BrowserStats(BrowserTelemetry),
    SetMode(AdaptationMode),
    SetCeiling(StreamSettings),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum HostMessage { Status(ControllerUpdate) }

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset { Performance, Balanced, Quality }

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StreamSettings {
    pub preset: Preset,
    pub fps: u32,
    pub bitrate_mbps: u32,
}
```

- `BrowserTelemetry` fields: `version: u8`, `sequence: u64`, `sampled_at_ms: u64`, plus optional `received_bitrate_bps: u64`, `packets_lost: i64`, `jitter_ms: f64`, `frames_decoded: u64`, `frames_dropped: u64`, and `rtt_ms: f64`. Optional values serialize as `null` or are omitted, never as zero by default. `sampled_at_ms` is monotonic only within that browser page session; compare it only with previous client sample times. Host freshness uses message-arrival `Instant`, never browser/host clock comparison.
- `HostMetrics` is host-only: `observed_at: Instant`, `capture_receive_interval: Option<Duration>`, `resize_time: Duration`, `color_convert_time: Duration`, `encode_time: Duration`, `queue_wait: Option<Duration>`, `queue_saturated: bool`, `cadence_missed: bool`, and `frame_age_since_capture_delivery: Duration`. The first frame reports `queue_wait: None`, because there is no prior completed send to measure; do not convert that missing signal to zero.
- `PressureReason` variants: `Stable`, `NetworkPressure`, `HostPressure`, `CombinedPressure`, `BrowserFeedbackUnavailable`, `HostFeedbackUnavailable`, `AtLowestTier`, `ApplyFailed`.
- `ControllerUpdate` contains `mode: AdaptationMode`, `ceiling: StreamSettings`, `effective: StreamSettings`, `reason: PressureReason`, `browser_feedback_available: bool`, `host_feedback_available: bool`, and `error: Option<String>`.
- `AdaptiveController` methods: `new(ceiling: StreamSettings, mode: AdaptationMode) -> Self`, `observe_browser(sample: BrowserTelemetry, received_at: Instant) -> ControllerUpdate`, `mark_browser_unavailable() -> ControllerUpdate`, `observe_host(sample: HostMetrics) -> ControllerUpdate`, `set_mode(mode: AdaptationMode) -> ControllerUpdate`, `set_ceiling(ceiling: StreamSettings) -> ControllerUpdate`, `desired_settings() -> Option<StreamSettings>`, `report_apply_success(settings: StreamSettings) -> ControllerUpdate`, `report_apply_failure(message: String) -> ControllerUpdate`, and `current() -> ControllerUpdate`. `ControllerUpdate.effective` always means the last host-confirmed settings; at most one newest desired target may be pending application. On channel close, clear browser pressure/recovery evidence while preserving sequence history, host pressure, and confirmed/pending settings; keep network feedback unavailable until new telemetry arrives.
- `StreamSettings` remains `preset: Preset`, `fps: u32`, `bitrate_mbps: u32`. Every adaptive tier is at or below ceiling output size, FPS, and bitrate; fixed mode always selects the ceiling.
- `CaptureStreams` runtime control is `policy: tokio::sync::watch::Sender<StreamSettings>`; application acknowledgements use `applied: tokio::sync::watch::Receiver<Option<PolicyApplyResult>>`, where `PolicyApplyResult` carries the attempted `StreamSettings` and `error: Option<String>`. Watch channels carry only the latest policy or apply result, never frames or browser telemetry, so the capture thread never blocks while publishing an acknowledgement.
- Check `webrtc` 0.21.0 callback/message/send APIs and `openh264` 0.9.8 encoder recreation/keyframe behavior against the pinned dependencies before locking implementation signatures. If a dependency prevents a requirement, report the exact API constraint and retain the peer session without claiming unsupported behavior.

## Delegated Tasks

### Task 1: Shared controller model and module registration

**Worker lane:** Controller/model worker. Own only `src/adaptive.rs`, `src/quality.rs`, and `src/main.rs`.

**Depends on:** Approved design spec only.

**Produces:** All shared Rust declarations above, `AdaptiveController`, and the `mod adaptive;` registration needed to compile the crate.

- [ ] Add the module registration to `src/main.rs` and define serde types in `src/adaptive.rs` with the exact names, variants, and fields in the shared contract. The expected registration is `mod adaptive;` beside the existing module declarations. Add serde derives and snake-case preset serialization to the existing `Preset`/`StreamSettings` types so `SetCeiling` matches the browser JSON shape.
- [ ] Implement `StreamSettings` tier candidates in `src/quality.rs`, preserving existing offer parsing/output sizing. Each candidate uses the selected ceiling as an upper bound for preset dimensions, FPS, and bitrate; fixed returns the original value.
- [ ] Implement controller validation and state transitions in `src/adaptive.rs`: reject unsupported versions, non-finite or invalid metrics, non-increasing sequence/sample times, and stale host-arrival samples without mutating state; track browser and host pressure independently; use sustained downshift and longer one-tier recovery; report missing feedback and lowest-tier pressure.
- [ ] Keep the starting persistence/recovery/rate parameters in one named policy configuration visible through diagnostics. Label defaults provisional; do not imply they were tuned from sustained results.
- [ ] Run `cargo fmt --check` and `cargo check --locked`; return raw output and the exact public signatures to the orchestrator. Do not run tests or commit.

### Task 2: Capture and encode timing instrumentation

**Worker lane:** Capture metrics worker. Own only `src/capture.rs` for this task. The same lane owns Task 3 because both change the capture loop.

**Depends on:** Task 1 `HostMetrics` declaration.

**Produces:** Each `EncodedFrame` carries one `HostMetrics` sample. It reports processing timings and bounded-queue behavior without changing policy yet.

```rust
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub duration: Duration,
    pub metrics: crate::adaptive::HostMetrics,
    pub output_size: (u32, u32),
}
```

- [ ] Add the metrics field to `EncodedFrame` and populate resize, color conversion, encode, capture receive interval, frame age since capture delivery, and cadence status in `encode`/capture-loop code. Preserve the existing `resize_time`, `color_convert_time`, and `encode_time` fields because `src/main.rs` benchmark consumes them. Mark a cadence miss when processing age exceeds one configured frame budget or capture receive interval reaches two frame budgets; this is a host pipeline/capture-arrival signal only.
- [ ] Measure encoded-queue wait around the existing bounded `blocking_send`; because the send consumes the frame before its wait is known, attach that completed wait/saturation observation to the next frame's metrics (`queue_wait: None` on the first sample). Preserve queue capacity and continue waiting rather than discarding encoded H.264 frames.
- [ ] Keep raw-frame timestamps monotonic and describe frame age as age since capture API delivery; do not call it glass-to-glass or native capture latency.
- [ ] Run `cargo fmt --check` and `cargo check --locked`; return raw output and a field-to-measurement mapping. Do not run tests or commit.

### Task 3: Runtime capture cadence and encoder policy

**Worker lane:** Same capture metrics worker as Task 2. Own only `src/capture.rs` for this task.

**Depends on:** Task 1 `StreamSettings`; Task 2 `EncodedFrame.metrics`.

**Produces:** `CaptureStreams.policy: watch::Sender<StreamSettings>`, `CaptureStreams.applied: watch::Receiver<Option<PolicyApplyResult>>`, and a capture loop that applies the newest policy without restarting capture or the PeerConnection. `PolicyApplyResult` carries the attempted `StreamSettings` plus `error: Option<String>`; no error means the encoder accepted the settings and recovery keyframe path.

```rust
pub struct CaptureStreams {
    pub video: tokio::sync::mpsc::Receiver<EncodedFrame>,
    pub audio: Option<tokio::sync::mpsc::Receiver<EncodedAudio>>,
    pub policy: tokio::sync::watch::Sender<StreamSettings>,
    pub applied: tokio::sync::watch::Receiver<Option<PolicyApplyResult>>,
}
pub struct PolicyApplyResult { pub settings: StreamSettings, pub error: Option<String> }
```

- [ ] Initialize the policy watch from session ceiling settings and the result watch from `None` in `capture::start`; return their sender/receiver in `CaptureStreams`. Let the capture thread poll policy changes between raw frames and publish apply results with `send_replace` without blocking media.
- [ ] Keep source capture at ceiling FPS and use a monotonic next-eligible-frame schedule to skip only superseded raw frames for a lower effective FPS. Preserve the one-frame raw channel and report cadence misses in `HostMetrics`.
- [ ] Inspect pinned OpenH264 APIs, then apply changed preset/dimensions/bitrate through a controlled encoder recreation when required. On success or failure, publish a `PolicyApplyResult` to the result watch with `send_replace`; on failure retain prior encoder/settings. Ensure a new keyframe is emitted or requested before dependent delta frames resume.
- [ ] For each successfully applied policy, encode subsequent frames with that policy and preserve the current encoded queue and video receiver contract.
- [ ] Run `cargo fmt --check` and `cargo check --locked`; return raw output plus verified encoder/keyframe guarantees and remaining limitations. Do not run tests or commit.

### Task 4: Offer mode compatibility and data-channel acceptance

**Worker lane:** WebRTC transport worker. Own only `src/server.rs` and `src/rtc.rs` for this task. The same lane owns Task 5 so callback ownership stays consistent.

**Depends on:** Task 1 `AdaptationMode` and wire types.

**Produces:** Backward-compatible HTTP offer parsing and one browser-created control data channel accepted by each new session. This task only establishes bounded message ingress; Task 5 routes messages to the controller.

```rust
#[derive(Deserialize)]
struct Offer {
    // existing fields...
    mode: Option<crate::adaptive::AdaptationMode>,
}
// Older clients that omit mode select AdaptationMode::Adaptive.
```

- [ ] Add optional `mode` to the existing `Offer`, default omitted values to adaptive, and pass it to `rtc::answer` without changing authentication, capture startup, or HTTP signaling.
- [ ] Verify the pinned `webrtc` data-channel callback and message APIs, register the callback before `set_remote_description`, then accept the browser-created `questdisplay-control` channel beside the existing video/audio tracks. Do not create a replacement peer or alter the HTTP answer flow.
- [ ] Copy incoming message bytes into a bounded per-peer Tokio channel of capacity one; when full, replace/drop stale telemetry according to the newest-message policy rather than allowing an unbounded queue. Preserve mode/ceiling commands if necessary using a separate bounded control slot.
- [ ] Add per-session teardown behavior so data-channel closure marks browser feedback unavailable while the video task can continue.
- [ ] Run `cargo fmt --check` and `cargo check --locked`; return raw output, exact callback ownership, and the data-channel label/message limits. Do not run tests or commit.

### Task 5: Route session messages, controller updates, and host metrics

**Worker lane:** Same WebRTC transport worker as Task 4. Own only `src/rtc.rs` for this task; `src/server.rs` is complete in Task 4.

**Depends on:** Task 1 controller API; Task 2 frame metrics; Task 3 `CaptureStreams.policy`; Task 4 bounded ingress and channel handle.

**Produces:** A controller per session that processes browser samples and commands, applies its chosen settings through watch, and returns status over the same channel.

```rust
match message {
    ControlMessage::BrowserStats(sample) => {
        controller.observe_browser(sample, Instant::now())
    }
    ControlMessage::SetMode(mode) => controller.set_mode(mode),
    ControlMessage::SetCeiling(ceiling) => controller.set_ceiling(ceiling),
}
// On a new proposal; effective stays host-confirmed until apply acknowledgement.
if let Some(desired) = controller.desired_settings() {
    policy.send_replace(desired);
}
// Encode and send HostMessage::Status(update) on the accepted data channel.
```

- [ ] Create the per-session `AdaptiveController` from initial offer ceiling and mode; immediately apply and publish its initial `ControllerUpdate`.
- [ ] Route validated `ControlMessage` values from Task 4 to controller methods, using host `Instant::now()` as browser-message arrival time. Send only `desired_settings()` proposals through the watch sender; do not report them as effective until Task 3 returns a successful `PolicyApplyResult` and `report_apply_success` confirms them.
- [ ] Feed `EncodedFrame.metrics` from the video stream loop into `observe_host`; keep media sample writing and credential-revocation checks in place.
- [ ] Drain Task 3's apply-result watch; route successful results to `report_apply_success` and failures to `report_apply_failure`; keep last confirmed settings, publish `ApplyFailed` on failure, and keep video alive where possible. If an older proposal completes after a mode/ceiling change, reconcile it against the newest target and dispatch the replacement proposal.
- [ ] Bound host status sends (one current status plus newest replacement); if the data channel closes, call `mark_browser_unavailable()`, stop status sends, and continue host-only adaptation/video without acting on stale browser pressure.
- [ ] Run `cargo fmt --check` and `cargo check --locked`; return raw output and code locations showing tier changes do not close/rebuild the PeerConnection. Do not run tests or commit.

### Task 6: Browser data channel and receive-stat telemetry

**Worker lane:** Browser protocol worker. Own only `web/index.html` for this task. The same lane owns Task 7 so the inline page remains coherent.

**Depends on:** Task 1 JSON tags/fields and `HostMessage::Status` shape; Task 4 channel label. It can be implemented in parallel with Task 5 once the wire contract is fixed.

**Produces:** Browser creates the control channel before offer generation and sends bounded one-second `BrowserTelemetry` while connected.

```js
const control = pc.createDataChannel('questdisplay-control');
const message = { type: 'browser_stats', data: {
  version: 1, sequence: ++state.telemetrySequence,
  sampled_at_ms: Math.round(performance.now()),
  received_bitrate_bps: bitrateBps ?? null,
  packets_lost: inbound?.packetsLost ?? null,
  jitter_ms: inbound?.jitter == null ? null : inbound.jitter * 1000,
  frames_decoded: inbound?.framesDecoded ?? null,
  frames_dropped: inbound?.framesDropped ?? null,
  rtt_ms: rttMs ?? null
}};
```

- [ ] Create `questdisplay-control` before `createOffer()`, include current `mode` in `/api/offer`, and bind `onopen`, `onmessage`, and `onclose` handlers to the active connection serial so stale callbacks cannot mutate a replacement session.
- [ ] Extend the existing one-second `getStats()` sampler to derive available counter deltas, received bitrate, jitter, decoded/dropped frames, and selected-pair RTT. Send absent values as `null`; never synthesize zero for a missing report.
- [ ] Keep only the newest pending browser telemetry sample while connecting or the data channel is not open. Limit serialization/send cadence to the existing one-second stats interval.
- [ ] Parse `HostMessage::Status` using the exact Task 1 snake-case tagged JSON format and pass it to a single UI update function created in Task 7; keep RTT labeled as RTT.
- [ ] Perform a manual local page inspection for offer/channel lifecycle and run `cargo check --locked` only after integrated work; do not run tests or commit.

### Task 7: Adaptive/fixed controls and effective-state UI

**Worker lane:** Same browser protocol worker as Task 6. Own only `web/index.html`.

**Depends on:** Task 1 `ControllerUpdate` serialization; Task 6 data-channel lifecycle and its status-dispatch hook. Task 7 can proceed in parallel with Task 5 once Task 1's wire contract is fixed.

**Produces:** Adaptive default, fixed fallback, live mode/ceiling changes, truthful ceiling/effective readouts, and clear degraded/restart-required status.

```js
function renderAdaptiveStatus(status) {
  ceilingText.textContent = formatSettings(status.ceiling);
  effectiveText.textContent = formatSettings(status.effective);
  stateText.textContent = status.error || pressureLabel(status.reason);
  feedbackText.textContent = feedbackLabel(status.browser_feedback_available,
                                           status.host_feedback_available);
}
```

- [ ] Add an Adaptive/Fixed selector defaulted to Adaptive and readouts for selected ceiling, effective preset/FPS/bitrate, current pressure reason, and browser/host feedback availability.
- [ ] On mode selection, send `{ type: 'set_mode', data: mode }`; on quality/FPS/bitrate changes, send `{ type: 'set_ceiling', data: settings }`. Update effective state only from host status, not optimistically from control input.
- [ ] Keep current-session controls from calling `connect()` when the data channel can apply them. If channel is unavailable or host rejects a command, show restart-required/degraded status and retain the last confirmed effective settings.
- [ ] Map every `PressureReason` variant from Task 1 to concise user-facing copy, including combined pressure, missing browser feedback, missing host feedback, lowest tier, and apply failure. Do not claim near-instant or numerical latency.
- [ ] Inspect embedded inline-script CSP hashing behavior in `src/server.rs`; hashes are computed dynamically from the served page. Perform a manual browser layout/control review at a narrow and wide viewport; do not run automated UI or Rust tests and do not commit.

### Task 8: Documentation updates and integrated manual review

**Worker lane:** Documentation worker owns only `docs/architecture.md`, `docs/performance.md`, and `docs/implementation-status.md`. The orchestrator owns the integrated review and permissible manual checks.

**Depends on:** Tasks 1–7 plus raw `cargo fmt --check`/`cargo check --locked` output and any available local manual-check evidence.

**Produces:** Documentation aligned with actual implementation and evidence.

- [ ] Update architecture flow: browser `getStats()` -> control data channel -> host controller -> latest-value policy -> capture/encoder, with status returning to browser. Retain the bounded encoded H.264 queue invariant.
- [ ] Extend the performance inventory with controller inputs/missing inputs/reasons/transitions, capture/encode/queue timings, and browser receive/decode metrics. State that none of RTT, encode duration, or receive stats alone proves glass-to-glass latency.
- [ ] Update implementation-status rows only for behavior actually implemented and checked. Keep Quest playback, physical-client keyframe recovery, controlled-network sustained results, and glass-to-glass latency unverified unless separately measured.
- [ ] Review every spec acceptance criterion against code and available evidence. Manually confirm old-offer mode default, fixed-mode invariance, channel-degraded behavior, bounded telemetry, no PeerConnection replacement on tier change, and stable signaling/playback where environment permits. Label unavailable physical Quest, network impairment, or browser checks as unverified.
- [ ] Run `cargo fmt --check` and `cargo check --locked` after integration and capture raw output. Do not run automated tests or commit.

## Task Dependency Graph

```text
Task 1 shared types/controller
  ├── Task 2 capture metrics ── Task 3 runtime policy application ──┐
  ├── Task 4 offer + data-channel ingress ──────────────────────────┤
  │                                      └── Task 5 session routing ┤
  └── Task 6 browser protocol ── Task 7 browser controls/status ────┤
                                                                    Task 8 docs + manual review
```

## Self-Review Coverage

- Spec goals/data flow, adaptive default, fixed mode, and no session replacement: Tasks 1, 3, 5, 6, and 7.
- Browser signals, unknown values, stale/malformed data, bounded telemetry: Tasks 1, 4, and 6.
- Host metrics, queue behavior, cadence, encoder reinitialization/keyframe recovery, and failure fallback: Tasks 2, 3, and 5.
- Independent pressure classification, persistence, slower recovery, ceiling enforcement, and lowest-tier reporting: Task 1; integration in Task 5.
- User diagnostics, documented evidence, and glass-to-glass limitation: Tasks 7 and 8.
- Interfaces are defined once above and repeated where consumers need exact Rust/JSON/runtime shapes; module registration belongs explicitly to Task 1.
- This plan adds/runs no tests or commits, per task instruction. Permitted verification is formatting/build checks and labeled manual checks only.
