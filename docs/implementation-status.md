# Implementation status

Updated 2026-09-28. This file describes checked-in code, not the product brief. Status values are **Implemented + tested**, **Implemented + source/build checked; runtime verification pending**, **Implemented + awaiting physical-platform verification**, **Blocked by documented OS limitation**, and **NOT IMPLEMENTED**. The Rust unit suite passed 14/14 on Linux x86_64, and Clippy passed with warnings denied before the adaptive-stream change. For the adaptive-stream change, `cargo fmt --all -- --check` and `cargo check --locked` passed; no automated tests were run. Chromium paired and decoded a real Linux Wayland desktop stream. After tuning, the 120-frame host benchmark measured 41.1 fps at 1152×720 Performance and 20.7 fps at 1728×1080 Balanced; final live Chromium UI readings were about 43 and 23 fps. A generated Linux desktop tone reached the live Chromium audio track through Opus. Audible speaker output, Quest playback, and A/V synchronization were not tested. No macOS or Windows playback test exists. The new adaptive controls, control-channel exchange, and live policy transitions have not been exercised in a browser or on a physical client; no near-instant or glass-to-glass result is established.

For the GPUI dashboard change on macOS arm64, `cargo check --locked --all-targets`, `cargo build --locked`, `cargo fmt --all -- --check`, and `cargo test --locked` passed (20 tests). The added backend tests cover confirmed listening, bind failure, port release after stopping, cancellation before startup finishes, private pairing expiry/redaction, and bounded session history. Clippy completed with warnings from existing adaptive/virtual-display code; the strict warnings-denied lint is not green on this platform. GUI rendering, live capture, and physical Quest use still need runtime verification.

## Core and transport

| Capability | Status | Evidence or remaining work |
| --- | --- | --- |
| Rust host and portable module boundaries | Implemented + tested | `cargo test --locked` passed 14/14 on Linux x86_64; Clippy passed with warnings denied. |
| Embedded web assets and HTTPS server | Implemented + tested | Live Chromium loaded the HTTPS page; HTTP/2 status returned 200. |
| Automatic HTTPS certificate and private-LAN bind | Implemented + tested | Live HTTPS served the browser; public bind rejection has a unit test. Certificate trust on Quest remains untested. |
| Host IP display and terminal QR code | Implemented + awaiting physical-platform verification | Code prints URL, QR, and certificate fingerprint; terminal rendering not checked across platforms. |
| mDNS name | NOT IMPLEMENTED | Use IP URL. |
| Browser-to-host WebRTC offer signaling | Implemented + tested | Live Chromium sent an offer, received an answer, and connected. |
| H.264 desktop stream and browser playback | Implemented + tested | Chromium displayed the real Hyprland desktop. Final release smoke had video readyState 4 at 1728×1080 and 1152×720, with instantaneous UI readings near 23 and 43 fps. |
| ICE without STUN/TURN for same LAN | Implemented + awaiting physical-platform verification | Live same-host Chromium WebRTC connected with empty ICE-server configuration. A separate LAN device has not been tested. |
| Browser reconnect | Implemented + awaiting physical-platform verification | Client logic exists; no interruption/reconnect test. |
| Capture denial keeps browser pairing | Implemented + awaiting physical-platform verification | Server maps capture refusal to HTTP 422; browser clears pairing for 401/403 only. This fix compiles and has tests, but no post-fix live denial test. |
| Multiple simultaneous clients | NOT IMPLEMENTED | No verified test. |
| Host-code pairing | Implemented + tested | Unit tests cover single use, peer binding, cooldown, and attempt limit; live Chromium paired through the API. CLI pairing requires an interactive terminal; GUI pairing goes only through a private in-process channel. |
| Browser bearer credential authorization | Implemented + tested | Live browser received a token and accessed the protected display list. |
| Credential persistence across host restart | Implemented + awaiting physical-platform verification | Code stores a SHA-256 token digest in native config; restart test pending. |
| Paired-device IDs and revoke commands | Implemented + tested | CLI lists digest-derived IDs. In isolated live Chromium smoke, `revoke <id>` changed protected API access from 200 to 401 and reset browser pairing; server logged stream revocation. A separate isolated CLI test used `revoke-all` on 3 stored credentials, then `devices` reported none. Concurrent-server behavior for `revoke-all` was not tested. |
| Private-peer, Host, and Origin guards | Implemented + tested | Unit tests cover public bind and header checks; live HTTP/2 returned 401 for unauthenticated displays and 400 for a malicious Host, and HTTP/1.1 returned 403 for a wrong Origin POST. |
| Full CSRF, DNS rebinding, replay, WAN threat-model validation | NOT IMPLEMENTED | Guard code exists, but broader adversarial test and review are absent. |
| No cloud, accounts, or telemetry | Implemented + tested | No such integration in the current source/dependency list; audit again before release. |

## Capture, media, and client

| Capability | Status | Evidence or remaining work |
| --- | --- | --- |
| Linux Wayland portal and PipeWire capture | Implemented + tested | On Hyprland, selecting a monitor in the xdg-desktop-portal-wlr picker delivered real desktop frames. Initial D-Bus timeout occurred while picker selection remained pending. |
| Wayland/X11 and virtual-compositor capability detection | Implemented + tested | `doctor` identifies session type and reports Hyprland/Sway IPC capability; Hyprland `virtual status` passed live. |
| Full portal, capture, audio, hardware-encoder, firewall detection | NOT IMPLEMENTED | `doctor` does not probe these facilities or produce the requested comprehensive readiness report. |
| Linux Hyprland capture | Implemented + tested | One live portal/PipeWire-to-Chromium session succeeded on this host. |
| Linux KDE, GNOME, Sway Wayland capture | Implemented + awaiting physical-platform verification | PipeWire portal adapter exists; no test on these compositors. |
| Linux X11 capture | NOT IMPLEMENTED | No X11-specific capture backend or verified portal route. |
| macOS ScreenCaptureKit capture and permissions | Implemented + awaiting physical-platform verification | Cross-platform `scrcap` adapter calls its macOS backend; no Mac build or permission test. |
| Windows Graphics Capture and permissions | Implemented + awaiting physical-platform verification | Cross-platform `scrcap` adapter calls its Windows backend; no Windows build or picker test. |
| Real display selection and live switching | NOT IMPLEMENTED | Client exposes `primary` only. Linux `scrcap` relies on portal picker and ignores a numeric monitor index; no verified source change without a new portal selection. |
| Software H.264 fallback | Implemented + tested | Browser decoded a real OpenH264 desktop stream on Linux; after tuning, host benchmark measured 41.1 fps Performance and 20.7 Balanced, with final browser UI near 43 and 23. |
| Hardware H.264: VA-API, NVIDIA, VideoToolbox, Media Foundation | NOT IMPLEMENTED | No native encoder backend. |
| GPU-native/zero-copy paths | NOT IMPLEMENTED | No measured path. |
| Performance, Balanced, Quality presets | Implemented + tested | Aspect-preserving resize and even dimensions have unit tests. Final live switch worked: Performance 1152×720 at about 43 UI fps and Balanced 1728×1080 at about 23; native size was seen earlier in a debug build at about 2 fps, not a current release result. |
| Custom preset | NOT IMPLEMENTED | FPS and bitrate can be overridden, but there is no named Custom preset or arbitrary resolution control. |
| Browser H.264 codec negotiation | Implemented + tested | Chromium decoded the H.264 stream on Linux; Quest profile support remains untested. |
| Browser codec/resolution capability fallback | NOT IMPLEMENTED | No alternative codec or automatic resolution fallback. |
| Linux desktop audio and Opus WebRTC track | Implemented + tested | `--audio` requested native `scrcap` audio; same-host Chromium received live unmuted audio and video tracks, with `opus` in browser stats. A 2 s, 440 Hz PipeWire tone produced a WebAudio analyser peak of 0.018585 on the received track, then zero after 1.5 s silence. Unit tests include an Opus round trip. Unsupported capture fallback exists but lacks a live test. Sources with more than two channels use first L/R. Human listening and separate LAN playback were not tested. |
| macOS/Windows/Quest audio delivery | Implemented + awaiting physical-platform verification | Shared optional audio code exists; no Mac or Windows build/audio run, Quest browser, or separate headset playback test. |
| Browser audio mute/unmute | Implemented + tested | Live Chromium audio button enabled playback (`video.muted=false`) after the audio track arrived, then muted it. Audible output was not checked by a listener. |
| Measured A/V synchronization | NOT IMPLEMENTED | No timestamped physical A/V synchronization test or correction result. |
| Browser display UI and diagnostics shell | Implemented + tested | Static Chromium render passed at 1280×800; live page showed pairing, video, and stats. |
| Browser live decode/RTT stats | Implemented + tested | Final same-host UI showed about 43 fps Performance and 23 fps Balanced, both with about 1 ms RTT. These are instantaneous readings; RTT is not screen-to-eye latency. |
| Browser fullscreen | Implemented + awaiting physical-platform verification | Control exists; fullscreen interaction not tested. |
| Browser quality preset control | Implemented + source/build checked; runtime verification pending | Preset/FPS/bitrate controls now update the selected ceiling over the control channel without reconnecting when supported. Existing live preset evidence used portal re-selection and does not verify the new live-command path. |
| Browser FPS and bitrate overrides | Implemented + awaiting physical-platform verification | Client sends values in offer and server validates ranges; no live override result recorded. |
| Browser pointer lock | NOT IMPLEMENTED | No remote input mode or pointer-lock flow. |
| Adaptive/fixed controller and tier hysteresis | Implemented + source/build checked; runtime verification pending | Adaptive is the default; fixed mode holds the selected ceiling. The controller uses separate network/host pressure inputs, sustained downshift, slower one-tier recovery, and an effective-settings apply acknowledgement. Starting thresholds are provisional. No automated controller tests or sustained pressure/recovery runs were performed. |
| Browser-host control channel and effective-state UI | Implemented + source/build checked; runtime verification pending | The browser creates a bounded `questdisplay-control` channel, sends optional one-second receive telemetry and mode/ceiling commands, and reads host-confirmed effective settings and pressure status. Browser tooling was unavailable for live layout/interoperability review; channel failure, telemetry behavior, and control acknowledgements remain runtime-unverified. |
| Runtime capture FPS/encoder policy | Implemented + source/build checked; runtime verification pending | Capture consumes latest-value policy updates, gates raw frames for lower FPS, and acknowledges encoder replacement only after producing an IDR. Requests above the session capture FPS require restart. No physical decoder/keyframe recovery or sustained adaptation was measured. |
| Browser bitrate, loss, RTT, decode/drop metrics | Implemented + source/build checked; runtime verification pending | Existing same-host `getStats()` UI showed decode rate and RTT in one live stream. The new optional telemetry message includes received bitrate, loss, jitter, decoded/dropped-frame counters, and candidate-pair RTT; the data-channel exchange has not been observed in a live browser session. These values do not measure screen-to-eye latency. |
| Host capture/encode/queue metrics | Implemented + source/build checked; runtime verification pending | Source records capture receive interval, resize/color-conversion/encode duration, cadence, frame age since capture delivery, and completed encoded-queue wait/saturation. Queue wait is reported on the next frame and starts unknown. These timings are not a validated end-to-end timing pipeline. |
| `benchmark` measured report | Implemented + tested | After color-conversion tuning, release command measured 120 static-desktop frames on Hyprland: Performance 41.1 FPS and Balanced 20.7 FPS, with stage timings in [performance](performance.md). Capture API time and glass-to-glass latency are outside its scope. |

## Input and virtual workspace

| Capability | Status | Evidence or remaining work |
| --- | --- | --- |
| Remote input data channel and normalized coordinate protocol | NOT IMPLEMENTED | No authenticated input path. |
| Linux uinput, macOS CGEvent, Windows SendInput | NOT IMPLEMENTED | No injection backends. |
| View-only mode | Implemented + tested | `/api/status` advertises `view_only`; no input injection path exists. |
| Mouse and keyboard control permissions | NOT IMPLEMENTED | No input backend, setting, or visible control indicator. |
| Quest browser pointer events and controller-to-host input | NOT IMPLEMENTED | No remote input path. XR controller controls only local screen placement. |
| Hyprland virtual-output CLI | Implemented + tested | On one Hyprland host, status was available, create added 1920×1080@60 `QUESTDISPLAY-*` alongside eDP-1, list showed it, and Ctrl-C removed it. |
| Sway virtual-output CLI | Implemented + awaiting physical-platform verification | `swaymsg` create/list/remove path has unit tests but no live Sway test. |
| KDE/GNOME/X11 virtual output and browser selection of Linux virtual output | NOT IMPLEMENTED | No adapters for those desktops; client accepts `primary` only. Hyprland portal reported source types 3 with no VIRTUAL bit; headless-output frame capture was not confirmed. |
| Windows IddCx driver, installer, signing, uninstall | NOT IMPLEMENTED | No driver source or package. |
| macOS virtual display | Blocked by documented OS limitation | No public general-purpose host virtual-display API identified; see [virtual displays](virtual-displays.md). This is not proof that one cannot be built. |
| View-only WebXR AR/VR screen, recenter, distance, scale, curvature | Implemented + awaiting physical-platform verification | WebGL2 video-texture renderer and feature detection are in the browser client; no Quest or other XR headset test. AR passthrough depends on browser support. |
| Multiple virtual screens | NOT IMPLEMENTED | Architecture target only. |

## Delivery and verification

| Capability | Status | Evidence or remaining work |
| --- | --- | --- |
| `start`, `gui`, `devices`, `displays`, `config`, `doctor` command parsing | Implemented + awaiting physical-platform verification | Commands compile; doctor and display/device output are basic. Runtime smoke pending. |
| `stop` and `status` commands | NOT IMPLEMENTED | No service manager or command handlers. |
| Native config directories and first-run certificate | Implemented + awaiting physical-platform verification | Code saves TOML and certificate; no fresh-machine permission test. |
| User settings for listen, port, FPS, bitrate, display | Implemented + awaiting physical-platform verification | TOML fields, CLI listen/port overrides, and GUI settings editing exist. GUI changes apply after stopping and starting the host. |
| Audio opt-in setting | Implemented + tested | A live Linux release host started with `--audio`; default is off. On unsupported native capture, code falls back to video only, but that fallback lacks a live test. |
| User settings for input, adaptive quality, virtual output | NOT IMPLEMENTED | No corresponding settings. |
| Automatic setup and interactive permission guidance | NOT IMPLEMENTED | Certificate, QR, and portal prompts exist, but no verified fresh-machine flow. |
| GPUI host dashboard and embedded runtime | Implemented + source/build checked; runtime verification pending | `gui` opens Overview, Devices, Settings, and Diagnostics. Start/stop runs HTTPS/WebRTC in-process; codes reach the GUI through private local state, and fresh host/browser telemetry is labeled by source. The native capture picker is not cancellable by the library: dismiss/complete an already-open picker to finish shutdown. No Linux/Windows/Quest dashboard run has been recorded. |
| Tray/menu-bar host interface | NOT IMPLEMENTED | A desktop window exists; no tray/menu-bar integration. |
| Structured logs with secret redaction | NOT IMPLEMENTED | No log audit. |
| Rust unit tests and lint | Implemented + tested | `cargo test --locked`: 14/14 passed on Linux x86_64, including Opus round trip; `cargo fmt --all -- --check` and Clippy with `-D warnings` passed. |
| Local Linux x86_64 optimized binary and archive | Implemented + tested | The final local `dist/questdisplay-linux-x86_64.tar.gz` contains the latest unsigned binary, README, LICENSE, and docs. It has not been installed on a fresh machine. The inspected release binary dynamically links libpipewire-0.3 and libdbus, but not libopus. |
| Static browser rendering | Implemented + tested | Chromium 1280×800 render passed. |
| HTTPS, authentication, and signaling integration smoke | Implemented + tested | Live HTTP/1.1 and HTTP/2 guards passed; Chromium paired, fetched displays, and sent an offer. |
| Linux WebRTC answer, real-frame, and browser playback integration smoke | Implemented + tested | Monitor chosen in portal picker; Chromium video readyState 4 with 34 decoded frames and zero dropped at inspection. |
| Linux, macOS, Windows CI builds | Implemented + awaiting physical-platform verification | Compile/test and tag archive workflows exist; no hosted CI run has been observed. |
| Unsigned Linux x86_64, macOS arm64/x86_64, Windows x86_64 executable archives | Implemented + awaiting physical-platform verification | Tag workflow configured; no tag build or downloadable archive observed. Linux binary is dynamically linked. |
| Linux aarch64 release archive | NOT IMPLEMENTED | No runner/target job configured. |
| AppImage/deb/rpm/Arch package, macOS app/dmg, Windows installer | NOT IMPLEMENTED | No native installer or package build. |
| Signed/notarized artifacts and fresh-machine installation | NOT IMPLEMENTED | Needs release credentials and target machines. |
| Physical Quest 3 playback and WebXR validation | NOT IMPLEMENTED | No device in this environment. Adaptive playback and encoder-transition recovery are also unverified on Quest. |
| Measured 1080p60, 1440p60, 1440p90, sub-50 ms | NOT IMPLEMENTED | Goals not met. Final same-host UI showed about 43 fps at 1152×720 and 23 at 1728×1080; no glass-to-glass measurement. |

The remaining manual verification steps are recorded in [Quest testing](quest-testing.md) and the platform guides. No performance target is represented as achieved.
