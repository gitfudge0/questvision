# Adaptive stream design

## Context

The host currently chooses capture FPS, output preset, and OpenH264 bitrate when a stream starts. The browser samples WebRTC receive statistics every second, but those statistics do not currently guide the host. The peer connection and video track remain active for the session.

The raw capture channel holds one frame and can discard stale frames when encoding falls behind. The encoded H.264 channel is bounded and blocks when full; dropping arbitrary encoded delta frames can break decoder references until a later keyframe. The pipeline uses software encoding. Recent same-host Chromium measurements in [performance](../../performance.md) show that output resolution and software encode load affect throughput, but do not establish sustained network behavior or glass-to-glass latency. In particular, WebRTC RTT is not end-to-end latency.

## Goals

- Adapt the running stream to both network conditions and host encode/queue pressure.
- Balance image quality and responsiveness while conditions are healthy. When poor conditions persist, favor smooth playback and low lag by stepping down stream load.
- Make adaptive mode the default while preserving an exact-settings fixed mode.
- Keep the existing peer connection and video session alive during adaptation.
- Expose enough status and diagnostics to explain the selected effective tier and tune the policy from measurements.

## Non-goals

- Guarantee a glass-to-glass latency or claim that the stream feels local on every network or device.
- Add hardware encoding, a new codec, multiple display streams, or a new signaling service.
- Treat RTT, encode duration, or browser statistics alone as end-to-end latency.
- Set final numeric controller thresholds before diagnostics and sustained tests are available.

## Proposed data and control flow

1. The browser continues sampling `RTCPeerConnection.getStats()` at its existing cadence. It sends a compact, timestamped summary over a WebRTC data channel on the existing peer connection. The summary should include the available receive-side signals needed by the controller, such as received bitrate, loss, jitter, decoded and dropped frames, and candidate-pair RTT. Missing fields are represented as unavailable rather than as zero.
2. The host combines the latest browser summary with local capture, encode, and queue measurements. Network pressure and host pressure remain separate inputs so a slow encoder is not misdiagnosed as a network problem, and vice versa.
3. A host-side controller chooses an effective tier within the configured ceiling and applies the associated runtime policy to output dimensions, frame cadence, and bitrate. Policy changes do not renegotiate or replace the PeerConnection.
4. The host reports the effective tier and current pressure reason back to the browser over the same data channel. The browser shows the selected ceiling, effective tier, and whether adaptation is active or telemetry is degraded.

The data channel is control telemetry, not media transport. Updates should be compact and bounded; newer browser summaries can supersede stale unsent summaries. The controller must reject stale or malformed samples and should not allow a telemetry burst to build an unbounded queue.

## Mode and client behavior

Adaptive mode is selected by default. The existing quality preset, FPS, and bitrate controls define the maximum settings the controller may use for that session. Adaptive mode may step below those ceilings when persistent network or host pressure warrants it, and may recover toward them after stable conditions.

Fixed mode preserves current exact-settings behavior: the chosen preset, FPS, and bitrate are used without the adaptive controller changing them. Switching modes applies to the active session without replacing its peer connection when the runtime controls permit it; otherwise the UI must clearly say that a restart is needed rather than silently presenting an unapplied choice.

The client displays both the configured ceiling and effective tier, plus a concise status such as adapting for network pressure, adapting for host load, stable, or browser feedback unavailable. This avoids presenting the selected preset as the actual current output when the controller has stepped down.

## Controller policy

Use a small ordered quality ladder bounded by the selected ceiling. A tier describes a coordinated set of output dimensions, maximum frame cadence, and encoder bitrate. The controller evaluates network pressure and host pressure independently, then selects the lower tier required by either signal. Network pressure may be indicated by sustained loss, jitter, receive/decode drops, or a receive rate that cannot sustain the current target. Host pressure may be indicated by sustained encode time, capture-to-encode backlog, encoded-queue wait, or missed output cadence.

The controller steps down only after pressure remains above a configured condition for a sustained interval. It recovers one step at a time only after a longer stable interval. This hysteresis limits oscillation and makes recovery slower than degradation. A worsening trend can trigger subsequent step-downs while the stream remains behind its ceiling. Network and host pressure durations, recovery conditions, tier mapping, and any rate limits must be centralized and visible to diagnostics; their numeric values are to be tuned from measurements, not assumed in this design.

The policy should prioritize reducing work that is causing delay. Host encode pressure should first reduce encoder load through an appropriate lower tier. Network pressure should lower the media demand to a level the receive path can sustain. If pressure persists at the lowest tier, retain the session and report the remaining limitation instead of promising smoothness.

## Runtime and encoder constraints

The raw one-frame capture channel remains the stale-frame boundary: it may discard superseded raw frames to keep encoding near the live desktop. The encoded H.264 queue remains bounded and must not drop arbitrary delta frames. Queue wait, saturation, and frame age need diagnostics so the controller can react before encoded backlog becomes visible lag.

The `openh264` 0.9.8 high-level `Encoder::encode` supports frames whose dimensions change and reinitializes when they do. Its public `Encoder` method list exposes no bitrate setter. A bitrate change may therefore require controlled encoder recreation and a fresh keyframe, while preserving the WebRTC PeerConnection and video track. This is a dependency API constraint, not a behavior validated in this repository. Tier transitions must account for encoder warm-up and keyframe cost, and must not claim that dynamic bitrate changes are already supported.

The `webrtc` 0.21.0 crate provides data channel support. The control channel can be added alongside the existing media track; it does not replace HTTP offer/answer signaling. See the [`webrtc` data channel API](https://docs.rs/webrtc/0.21.0/webrtc/data_channel/index.html) and [`openh264` Encoder API](https://docs.rs/openh264/0.9.8/openh264/encoder/struct.Encoder.html).

## Degraded and error behavior

- If the data channel cannot be established, keep the video session running. Adaptive mode can respond to host-side pressure, but lacks browser receive feedback; show that limitation and do not claim full network adaptation.
- If browser stats are unavailable or incomplete, mark those signals unknown. Do not interpret absence as healthy conditions or make network-driven recovery decisions. Continue host-side adaptation; otherwise hold the current effective tier and report that browser feedback is unavailable.
- If host timing or queue diagnostics are unavailable, do not infer host health from browser RTT. Continue using available browser receive signals and report the missing host feedback.
- If adaptation or encoder reinitialization fails, retain the last known working settings where possible, report the error, and keep the peer session alive. Fixed mode remains available as the explicit no-adaptation fallback.
- Malformed, stale, or out-of-order telemetry must be ignored without destabilizing the stream.

## Acceptance criteria

- Under a sustained network-pressure test, the effective tier steps down within the configured persistence window without closing or replacing the PeerConnection; the browser reports network pressure as the cause.
- Under a sustained host encode or queue-pressure test, the effective tier steps down and diagnostics identify host pressure rather than network pressure.
- Short-lived pressure that does not cross the configured persistence condition does not cause a tier transition. Recovery occurs one tier at a time only after its longer stable condition, with no repeated up/down oscillation in a sustained stable run.
- The browser reports the selected ceiling and actual effective tier, and indicates when feedback is unavailable or incomplete.
- Fixed mode preserves the selected exact settings for the session. Existing signaling, pairing, and video playback continue to work when adaptive mode is enabled.
- Diagnostics record controller inputs, missing inputs, pressure classification, tier transitions, transition reason, capture/encode timing, queue wait/saturation, and relevant browser receive counters with monotonic timestamps.
- A live adaptation check confirms that resolution/FPS/bitrate policy changes do not require replacing the PeerConnection. Any encoder recreation path confirms that decoding resumes after the required keyframe.
- Measurements report receive/decode behavior and host pipeline timing separately from glass-to-glass latency. No latency target is considered demonstrated until measured on the intended physical client.

## Testing and physical measurements

Unit tests should cover signal classification, missing and stale statistics, sustained-pressure timing, slower recovery, ceiling enforcement, lowest-tier behavior, and fixed-mode invariance. Integration tests should exercise the data channel, runtime tier changes, encoder recreation and keyframe recovery, and failure fallback while confirming the peer connection remains active.

Run sustained tests with controlled network impairment and separate host-load pressure so the two causes can be checked independently. Record host OS and CPU, display size/refresh, browser and client version, Wi-Fi band/channel and access point, signal conditions, selected ceiling, effective tiers, and all controller transitions. Report median, p95, and worst observed results across repeated runs.

Quest glass-to-glass performance remains unverified. Measure it on a physical Quest using a high-contrast timing marker visible on both the host and headset, filmed together with a high-speed camera (or a synchronized photodiode setup). Record camera rate and analysis uncertainty. Existing performance figures are context for choosing diagnostics and tier candidates; they are not proof of end-to-end latency or a near-instant guarantee.
