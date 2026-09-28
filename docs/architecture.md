# Architecture

The host pipeline is native capture through `scrcap`, optional CPU resize, CPU color conversion, bundled OpenH264 encoding, and a WebRTC H.264 video track. The embedded HTTPS server serves the client and handles authenticated HTTP signaling. A new WebRTC peer session starts its own capture and encoder. The raw capture channel holds one frame, so `scrcap` can discard stale raw frames and the encoder can skip superseded raw frames when its active FPS is below the capture ceiling. The encoded H.264 queue is bounded but waits when full: discarding arbitrary delta frames would break decoder references until another keyframe. This path is CPU based and is not zero-copy.

```text
Browser --stats and controls--> control data channel --> host controller
                                                         | latest policy
                                                         v
native capture -> one-frame raw channel -> CPU resize/convert -> OpenH264 -> bounded encoded H.264 queue -> video track -> Browser
Browser <--status over same channel <-- host controller <-- host metrics and apply ack <---------------- capture / encoder / queue

authenticated HTTPS signaling <-> browser (session setup)
```

The browser samples receive bitrate, packet loss, jitter, decoded/dropped-frame counters, and selected candidate-pair RTT once per second. Available values travel as compact versioned messages; missing values stay unknown. The host controller validates ordering and freshness, combines browser feedback with host capture/encode/queue measurements, and classifies network and host pressure separately. Adaptive mode is the default. The selected preset, FPS, and bitrate are the ceiling; sustained pressure can lower the effective tier, and recovery is slower. The starting thresholds and tier policy are provisional and require sustained measurements before tuning.

The controller sends only its newest desired policy through a Tokio watch channel. Capture applies it between raw frames. A changed encoder configuration is prepared separately and becomes effective only after an IDR frame is produced; the controller reports the last host-confirmed settings. Tier changes keep the existing PeerConnection and media track. Fixed mode holds the selected settings. If the browser control channel closes, video continues and host-only adaptation can continue, while stale browser pressure is cleared and feedback is shown as unavailable. Settings above the session's native capture FPS require reconnecting because the capture source itself was started at the offer FPS.

Host metrics describe frame processing after delivery from the capture API. Queue wait and saturation are observed after a bounded send completes and appear with the next frame, so the first queue-wait sample is unavailable. These values do not measure native capture time or glass-to-glass latency. RTT, browser receive/decode counters, and encoder timings must be read as separate diagnostics.

With `--audio`, `scrcap` also requests native desktop audio. The host encodes 48 kHz stereo Opus in 20 ms packets and adds a WebRTC audio track; unavailable audio falls back to video only. Audio starts disabled, and the browser requires a separate user action to enable playback. Sources with more than two channels use their first left and right channels. A generated test tone reached same-host Chromium over the live Linux stream; audible speaker output and A/V synchronization have not been measured.

The repository uses a single Rust package with capture, audio, config, WebRTC, security, and server modules. The implementation status file records which behaviors have been checked and which still need runtime or physical-platform evidence. Do not infer hardware encode, input, virtual output, or multiple displays merely from this diagram.

The browser receives a `MediaStream` in a `<video>` element. Signaling exchanges SDP through authenticated HTTP, with ICE candidates included after host-side gathering. LAN media travels through WebRTC. An experimental WebGL2/WebXR renderer uses that video as a texture when the browser exposes an immersive AR or VR session. Quest behavior is untested. Current code accepts only `primary`; multiple display streams need separate tracks or explicit source identifiers rather than assuming a peer owns exactly one monitor.

See [decisions](architecture-decisions.md) for dependencies and [performance](performance.md) for the measurement plan.
