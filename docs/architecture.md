# Architecture

The host pipeline is native capture through `scrcap`, optional CPU resize, CPU color conversion, bundled OpenH264 encoding, and a WebRTC H.264 video track. The embedded HTTPS server serves the client and handles authenticated HTTP signaling. A new WebRTC peer session starts its own capture and encoder. The raw capture channel holds one frame, so `scrcap` can discard raw frames if the encoder falls behind. The encoded H.264 queue is bounded but waits when full: discarding arbitrary delta frames would break decoder references until another keyframe. This path is CPU based and is not zero-copy.

```text
native capture -> one-frame raw channel -> CPU resize/convert -> OpenH264 -> WebRTC track
                            HTTPS/signaling/auth <-> browser
```

With `--audio`, `scrcap` also requests native desktop audio. The host encodes 48 kHz stereo Opus in 20 ms packets and adds a WebRTC audio track; unavailable audio falls back to video only. Audio starts disabled, and the browser requires a separate user action to enable playback. Sources with more than two channels use their first left and right channels. A generated test tone reached same-host Chromium over the live Linux stream; audible speaker output and A/V synchronization have not been measured.

The repository uses a single Rust package with capture, audio, config, WebRTC, security, and server modules. The implementation status file records which behaviors have been tested. Do not infer hardware encode, input, virtual output, or multiple displays merely from this diagram.

The browser receives a `MediaStream` in a `<video>` element. Signaling exchanges SDP through authenticated HTTP, with ICE candidates included after host-side gathering. LAN media travels through WebRTC. An experimental WebGL2/WebXR renderer uses that video as a texture when the browser exposes an immersive AR or VR session. Quest behavior is untested. Current code accepts only `primary`; multiple display streams need separate tracks or explicit source identifiers rather than assuming a peer owns exactly one monitor.

See [decisions](architecture-decisions.md) for dependencies and [performance](performance.md) for the measurement plan.
