# Architecture decisions

Reviewed 2026-09-27. These are implementation choices or design targets, with the distinction stated in each entry. They do not imply platform testing.

## ADR 1: Rust host, web client

**Decision:** Use a Rust host executable and embedded browser assets. Rust gives one cross-platform codebase for networking and state, while allowing native capture and encoder APIs through FFI. A browser client removes the need for a Quest installation. Electron, Node.js, FFmpeg, GStreamer, OBS, and cloud services are not planned runtime dependencies.

**Tradeoff:** Platform-specific capture, encoding, and permissions still need dedicated code and physical tests. A Rust executable alone does not guarantee a self-contained Linux package because PipeWire and desktop portal are OS services.

## ADR 2: Native capture through `scrcap`

**Decision:** Use [`scrcap` 0.1.0-alpha.2](https://docs.rs/scrcap/latest/scrcap/) as the capture adapter. It selects the portal and PipeWire on Linux, ScreenCaptureKit on macOS, and Windows Graphics Capture on Windows. Its frame channels and target picker reduce the amount of unsafe platform code needed for the first real stream.

**Why not implement all native bindings here:** The native APIs have distinct permission, lifetime, and frame ownership rules. An initial integration can be reviewed sooner with a small wrapper. `scrcap` is an alpha dependency; pin and audit it before a stable release. The capture adapter must expose the active backend, frame format, timestamps, and copy path. GPU-native zero-copy is not claimed.

**OS references:** [XDG ScreenCast portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html), [Apple ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit), [Microsoft Windows Graphics Capture](https://learn.microsoft.com/en-us/windows/uwp/audio-video-camera/screen-capture).

## ADR 3: WebRTC inside the host

**Decision:** Use the Rust [`webrtc` 0.21 crate](https://docs.rs/webrtc/latest/webrtc/) for the async peer connection and its [`rtc` 0.21 core](https://docs.rs/rtc/latest/rtc/) for codec, media sample, and RTP parameter types used directly by the host. `webrtc` itself builds on `rtc`; listing both as direct Cargo dependencies makes those type imports explicit. The library handles ICE, DTLS, SRTP, RTP, congestion control, and data channels. The application owns signaling over its own HTTP service. LAN mode has no configured public STUN or TURN server.

**Why not native implementation:** WebRTC and its cryptographic protocols are standards-heavy and security sensitive. The crate is compiled into the host. There is no separate service for users to install. Same-subnet host candidates are the intended path; routed LANs, guest Wi-Fi isolation, VPNs, and unusual NAT setups can fail without additional ICE infrastructure.

## ADR 4: Bundled OpenH264 first

**Decision:** Use the Rust [`openh264` crate](https://docs.rs/crate/openh264/latest) with its bundled source feature for the first H.264 sender. This gives one codec implementation across platforms while native hardware encoder work proceeds. It is software encoding. A native VideoToolbox, Media Foundation, VA-API, or NVENC path must be identified separately in diagnostics and never inferred from H.264 alone.

**Why not write a codec:** H.264 is far outside the safe scope of this project. Bundling avoids asking users to install FFmpeg or a codec process. CPU cost and quality at 1440p60 or 90 fps are unmeasured and may be too high.

**License and patents:** The wrapper and included OpenH264 source use BSD-2-Clause according to the [upstream crate notice](https://docs.rs/crate/openh264/latest/source/README.md). That copyright license does not settle all H.264 patent questions for every distributor or jurisdiction. Cisco's separate coverage for its own prebuilt binaries must not be assumed for locally compiled source. Release maintainers need a distribution-specific legal review before shipping binaries.

## ADR 5: Optional native audio and bundled Opus

**Decision:** Optional desktop audio is captured through `scrcap` and encoded as Opus at 48 kHz stereo in 20 ms packets for a WebRTC audio track. It is disabled unless the host starts with `--audio`, and the browser must enable playback. The host falls back to video only if native audio capture is unavailable. Sources with more than two channels use the first left and right channels.

**Tradeoff:** This avoids a virtual audio device and external codec installation. The codec is compiled into the application; building it from source requires CMake. Unit tests cover audio conversion and an Opus round trip. A generated tone reached same-host Chromium in a live Linux test, but audible speaker output, Quest playback, and A/V synchronization have not been measured.

## ADR 6: Local HTTPS and pairing

**Decision:** Generate a host key and a self-signed certificate on first run, serve HTTPS, and require host-displayed pairing for a browser credential. TLS and randomness come from maintained libraries, not custom cryptography. Disable remote input by default.

**Tradeoff:** A self-signed certificate causes a browser trust warning unless its certificate or issuing authority is installed and trusted. The warning cannot be removed honestly for an arbitrary private IP with no trusted authority. WebXR requires a [secure context](https://developer.mozilla.org/en-US/docs/Web/API/WebXR_Device_API/Startup_and_shutdown), and Quest Browser behavior after accepting a certificate exception still needs physical verification. The exact implemented pairing controls are tracked in [security](security.md).

## ADR 7: Virtual displays are optional

**Decision:** Keep virtual-display management separate from physical-display capture. No third-party virtual-monitor application is required for basic streaming. On Windows, the supported route is an [IddCx indirect display driver](https://learn.microsoft.com/en-us/windows-hardware/drivers/display/indirect-display-driver-model-overview), which needs a separately built, signed, explicitly installed component. On Linux, the portal's [VIRTUAL source type](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html) and compositor support need per-environment tests. We have not identified a public general-purpose macOS API for an arbitrary extended desktop display; [Paravirtualized Graphics](https://developer.apple.com/documentation/paravirtualizedgraphics/pgdisplay) applies to virtualization, not a normal host app.

**Status:** `questdisplay virtual` can manage a Hyprland headless output, verified on one host. Sway IPC code exists but has no live compositor test. The capture client cannot explicitly choose the new output; portal selection of it is unverified. There is no Windows driver. macOS support is an unresolved public-API limitation, not a claim of impossibility.

## Dependency classification

| Dependency | Class | Purpose | Native alternative rejected | Bundling and user install | Alternatives |
| --- | --- | --- | --- | --- | --- |
| PipeWire, portal, ScreenCaptureKit, Windows Graphics Capture | OS facility | Consent-based screen frames | None; these are native interfaces | Supplied by OS or desktop session | X11 capture, platform-specific API calls |
| `hyprctl` / `swaymsg` | Compositor facility, optional | Create/list/remove a headless Linux output through compositor IPC | Direct compositor protocols would add maintenance without removing the compositor requirement | Normally bundled with the matching compositor; no separate installation for basic streaming | Portal VIRTUAL source, direct IPC |
| `scrcap` | Embedded Rust crate, alpha | Native capture adapter | Three independent FFI backends would delay a working first stream | Compiled in; Linux still needs desktop services | Direct API bindings, `pinray` |
| `webrtc` and direct `rtc` core | Embedded Rust crates | Async WebRTC peer, codec and media types, protocol transport | Custom WebRTC/DTLS/SRTP is unsafe and costly | Compiled in, no user service | libwebrtc bindings |
| `openh264` | Embedded C/Rust library | Software H.264 fallback | Custom codec is inappropriate | Source compiled in; no codec installation | Native hardware encoders, Cisco prebuilt binary |
| [`opus` 0.4](https://docs.rs/crate/opus/0.4.0) and [`opusic-sys` 0.7.5](https://docs.rs/crate/opusic-sys/0.7.5) | Embedded Rust/C library | Encode optional desktop audio as Opus for WebRTC | A custom Opus codec would be unsafe and out of scope | Default bundled codec compiled into the host; CMake is needed only to build from source, not by users running the binary | OS codec interfaces, other Opus bindings |
| [`fast_image_resize` 6.1.0](https://docs.rs/fast_image_resize/latest/fast_image_resize/) | Embedded Rust crate | CPU downscale before software H.264 to reduce pixel load | Platform-specific GPU scaling is not yet integrated | Compiled in; no user installation | GPU blit/compute scaling, custom resize code |
| `axum`, `axum-server`, `tokio`, `serde`, `toml`, `clap` | Embedded Rust crates | HTTPS, async runtime, data formats, CLI | OS-specific replacements add code without value | Compiled in; none installed by users | Smaller custom code, other Rust crates |
| `rustls`, `rcgen`, `rand`, `sha2`, `subtle` | Embedded Rust crates | TLS, local certificate, randomness, hashes, constant-time equality | Custom cryptography is prohibited | Compiled in; none installed by users | OS TLS APIs |
| `qrcode`, `local-ip-address`, `dirs` | Embedded Rust crates | Terminal QR, LAN address choice, native config directory | Small cross-platform helpers are less fragile than platform-specific reimplementations | Compiled in; none installed by users | Manual URL, per-OS discovery code |

The exact dependency tree and license inventory must be generated from the lockfile for a release. This table lists significant direct dependencies and the bundled Opus native dependency, not every transitive crate. The `opus` wrapper declares MIT OR Apache-2.0; `opusic-sys` and bundled libopus declare BSD-3-Clause. Distribution must retain applicable notices and review these alongside H.264 licensing.
