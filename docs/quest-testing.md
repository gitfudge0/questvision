# Quest 3 test record

No physical Meta Quest 3 was available in this environment. No Quest OS or Browser version, H.264 profile, maximum resolution, maximum FPS, fullscreen result, browser throttling result, decoder behavior, WebXR result, Wi-Fi threshold, or latency figure has been recorded. Any later entry must include a device, date, method, and result.

Development host for the automated checks was Linux x86_64 under Hyprland Wayland, with PipeWire 1.6.9 and libva 1.24.0. The user PipeWire and xdg-desktop-portal services were active. Portal/PipeWire capture and software H.264 playback were tested locally; hardware encoding was not.

| Test | Current evidence | Result |
| --- | --- | --- |
| Latest Rust checks | Linux x86_64, 2026-09-27 | `cargo test --locked` 14/14, including an Opus round trip; Clippy with warnings denied and `cargo fmt --all -- --check` passed. A release binary with optional audio was used for a live browser test. |
| Linux desktop audio through browser track | Release host with `--audio`, same-host Chromium on Hyprland, 2026-09-27 | HTTPS pairing and portal selection succeeded after unlocking the desktop. Video was playing with readyState 4; MediaStream had live, unmuted audio and video tracks. Browser stats reported `opus`. The audio button changed `video.muted` to false, then muted it again. A 2 s, 440 Hz low-volume stereo WAV played with PipeWire `pw-play` produced a 0.018585 peak in a WebAudio analyser on the received track; 1.5 s after the tone, peak was zero. This proves signal arrived at the browser track, not that a person heard speaker output. No Quest, separate LAN client, or A/V sync test. Earlier locked-session portal timeout was an environmental test obstacle and resolved after unlocking. |
| Static desktop browser rendering | Chromium at 1280×800, 2026-09-27 | Page rendered; no live server connection |
| Desktop browser HTTPS, pairing, display list, offer | Chromium on Linux x86_64, live host, 2026-09-27 | Page loaded, pairing succeeded, protected display list loaded, WebRTC offer sent |
| Desktop browser video playback, early debug build | Chromium on the same Linux host, 2026-09-27 | Real desktop visible at 2880×1800; video readyState 4, paused false, 34 decoded frames and zero dropped at inspection. UI showed about 2 fps and 1 ms RTT. This is not current release performance. |
| Desktop browser preset samples before color-conversion tuning | Optimized release binary, AMD Strix/Hyprland, same-host Chromium, 2026-09-27 | Performance 1152×720: 152 decoded frames/5.0004 s = 30.40 fps, zero dropped. Balanced 1728×1080: 38/5.0003 s = 7.60 fps, zero dropped. Quality switch required portal re-selection. Not a Quest or latency result. |
| Post-tuning browser smoke | Same-host Chromium, AMD Strix/Hyprland, 2026-09-27 | Balanced 1728×1080 video readyState 4, paused false, UI about 23 fps and 1 ms RTT. Switched to Performance, selected portal monitor again, video readyState 4 at 1152×720, UI about 43 fps and 1 ms RTT. Instantaneous readings, not a timed benchmark. |
| Host release benchmark, static desktop | Linux Hyprland, 120 real captured/encoded frames per preset, 2026-09-27 | Performance 1152×720: resize 4.50 ms, conversion 0.54 ms, encode 18.57 ms, 41.1 FPS, 1.29 Mbps. Balanced 1728×1080: resize 7.87 ms, conversion 1.33 ms, encode 38.56 ms, 20.7 FPS, 0.59 Mbps. After color-conversion tuning; no Quest or glass-to-glass measurement. |
| Hyprland virtual-output lifecycle | Same Linux host, `questdisplay virtual`, 2026-09-27 | Status available; create added 1920×1080@60 `QUESTDISPLAY-*` next to eDP-1; list showed it; Ctrl-C removed it. Portal source types were 3, with no VIRTUAL bit. Picker test did not confirm headless-output frame delivery. |
| Browser credential revocation | Isolated live HTTPS host and Chromium, 2026-09-27 | Browser paired and `/api/displays` returned 200; separate CLI `devices revoke <id>` succeeded; old token then returned 401, browser cleared token and showed pairing, server logged stream closure. Already-playing video interruption timing was not measured. |
| Quest Browser H.264 baseline/main/high negotiation | Physical Quest required | Not tested |
| 1080p60 and 1440p60 stability | Physical Quest required | Not tested |
| 1440p90 and 4K60 limits | Physical Quest required | Not tested |
| Fullscreen, browser focus, throttling, decode stats | Physical Quest required | Not tested |
| Certificate exception and secure-context behavior | Physical Quest required | Not tested |
| WebXR AR/VR video texture, placement controls | Client code exists; physical Quest required | Not tested |
| WebXR controller-to-host input | Feature not implemented | Not tested |
| Quest audio playback and A/V synchronization | Physical Quest required | Not tested |
| Glass-to-glass latency | High-speed camera or equivalent required | Not measured |

## Physical test procedure

1. Record Quest OS version and Browser version from the device, plus host hardware and network setup.
2. Open the host URL on the Quest and record the certificate and pairing flow, including any blocked state.
3. Record the negotiated codec/profile and WebRTC candidate pair from browser diagnostics.
4. Test 1080p60 first. Log ten minutes of received/decoded FPS, dropped frames, packet loss, RTT, and host encode time.
5. Repeat at 1440p60 and 1440p90 only if the previous setting is stable. Test text legibility and rapid window motion.
6. Test fullscreen, headset sleep/wake, tab backgrounding, Wi-Fi interruption, reconnect, and display switching.
7. Measure latency with the method in [performance](performance.md). Record raw observations and uncertainty.

Do not replace this table with guessed hardware limits.
