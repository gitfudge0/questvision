# Performance and measurement

The live Linux/Chromium test on 2026-09-27 used an AMD Strix host running Hyprland and software OpenH264. These were same-host browser decode observations, not a Quest test or sustained benchmark:

| Setting | Output | Decoded frames / interval | Decoded FPS | Dropped at inspection |
| --- | --- | --- | --- | --- |
| Native size, early debug build before resize controls | 2880×1800 | 34 frames at inspection | About 2 from UI | 0 |
| Performance, optimized release build | 1152×720 | 152 / 5.0004 s | 30.40 | 0 |
| Balanced, optimized release build | 1728×1080 | 38 / 5.0003 s | 7.60 | 0 |

The native-size UI also reported about 1 ms WebRTC RTT. RTT is network round-trip time, not glass-to-glass latency. The host benchmark below measures resize, color conversion, and encode time, but not capture API time or glass-to-glass latency. 1080p60 and 1440p60 remain goals; 1440p90 and sub-50 ms glass-to-glass are stretch goals. The measured settings did not reach 60 fps.

The release `questdisplay benchmark` command measured 120 captured and encoded frames of a static desktop on the same Hyprland host after the latest color-conversion tuning:

| Preset | Output | Mean resize | Mean BGRA/RGBA to YUV | Mean H.264 encode | Captured/encoded FPS | Encoded bitrate |
| --- | --- | --- | --- | --- | --- | --- |
| Performance | 1152×720 | 4.50 ms | 0.54 ms | 18.57 ms | 41.1 | 1.29 Mbps |
| Balanced | 1728×1080 | 7.87 ms | 1.33 ms | 38.56 ms | 20.7 | 0.59 Mbps |

These are host pipeline measurements, not browser decode rates. The low encoded bitrates reflect static desktop content and do not establish quality during motion. Software encoding remains the dominant measured stage, and 1080p60 remains unmet. Capture API time, queue wait, and glass-to-glass latency were not measured. The timed browser samples in the first table preceded this color-conversion tuning. In a final live Chromium smoke after tuning, the UI showed about 43 fps at 1152×720 Performance and 23 fps at 1728×1080 Balanced, both with about 1 ms RTT. Those were instantaneous readings, not five-second decode counts.

Optional audio was tested for delivery, not latency: on the same Linux host, a two-second 440 Hz stereo WAV played through PipeWire reached Chromium's Opus track. A WebAudio analyser measured a 0.018585 peak during the tone and zero after 1.5 seconds of silence. The browser audio button enabled then muted playback. This does not measure audible output, audio latency, drift, or A/V synchronization.

The client offers Performance up to 1280×720, Balanced up to 1920×1080, and Quality at native size. These are output caps: aspect ratio is preserved and dimensions are rounded to even pixels. FPS can be set between 15 and 120; bitrate between 2 and 80 Mbps. The host resizes in process before software encoding. The short post-resize samples above show a large resolution effect, but they do not establish stable long-run throughput.

## Adaptive stream diagnostics

The adaptive path is implemented, but its controller settings are provisional. It records the policy and tier candidates at session start. The current defaults use 3 s persistence for network and host pressure, 12 s for recovery, a 3 s transition interval, and 4 s/3 s freshness windows for browser/host feedback. Pressure inputs include browser jitter, candidate-pair RTT, packet-loss deltas, decoded/dropped-frame deltas, and a received-bitrate plus decode-rate shortfall. Host inputs include resize, color-conversion and encode durations, capture receive interval, missed cadence, frame age since capture delivery, and bounded encoded-queue wait/saturation. Missing or stale fields remain unknown; they do not count as healthy observations.

Controller status reports the selected ceiling, last host-confirmed effective settings, separate browser/host feedback availability, pressure reason, and apply errors. The browser receives/decode figures and RTT remain separate from host timings. Queue wait is known only after a completed encoded-frame send and is attached to the next frame's host sample; the initial wait is unavailable. Frame age begins when the capture API delivers a frame to the host process, not when pixels changed on the display. No adaptive transition or threshold has yet been measured under sustained network impairment or sustained host load, so these initial values must not be treated as tuned recommendations or a smoothness guarantee.

## Measurement plan

Record host OS, CPU, GPU, display size and refresh rate, browser and Quest firmware version, Wi-Fi band/channel, AP model, and signal conditions. Run at least three sustained sessions per preset. Report median, p95, and worst observed latency rather than a single best frame.

At the host, record the available monotonic capture-delivery interval, resize, color conversion, encode, frame-age, cadence, and queue-wait/saturation observations. Queue wait is carried forward to the next frame because the bounded send consumes the frame before its blocking time is known. The raw capture channel has capacity one and can discard stale raw frames. The encoded H.264 queue blocks when full, because dropping reference-dependent encoded frames would corrupt playback. Queue behavior and transition impact still need sustained-load measurement. At the browser, use `RTCPeerConnection.getStats()` for received bitrate, packets lost, jitter, decoded frames, dropped frames, and candidate-pair RTT. None of RTT, encode duration, browser receive statistics, or any single host timing proves photon-to-photon latency.

Exercise the adaptive controller with sustained controlled network impairment, sustained host encode pressure, and combined pressure. Record status reasons, inputs and missing inputs, every effective-tier change, apply failures, and time from pressure onset to confirmed application. Check recovery separately and report median, p95, and worst observed results over repeated runs. Also verify that video continues if browser feedback disappears and that the same PeerConnection remains active across tier changes.

For glass-to-glass, show a high-contrast timing marker on the host screen and film both host and Quest displays with a high-speed camera in one shot. State camera frame rate and analysis uncertainty. A synchronized photodiode setup can provide better precision. Do not label network RTT or encode time as end-to-end latency.

`questdisplay benchmark --preset performance|balanced|quality` captures and encodes 120 real frames after the OS source picker and reports output size, frame rate, mean resize/color-conversion/H.264 encode times, and encoded bitrate. The Performance and Balanced results above are from this command. It explicitly does not measure capture API time or end-to-end latency, and it does not exercise adaptive decisions. The adaptive-quality ladder, browser control channel, host metrics, and effective-state status are implemented in source, but integrated browser behavior, sustained transitions, physical-client keyframe recovery, and Quest playback have not been demonstrated. A fuller benchmark should report capture-delivery, queue, and drop metrics alongside the active backend and memory path.

No measurement in this file demonstrates local-like or near-instant presentation. Measure glass-to-glass on the intended physical Quest using the method above before making a latency claim.
