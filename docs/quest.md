# Quest Browser

Quest 3 Browser is the target client, but there has been no physical Quest test. The page, H.264 decoder limits, fullscreen behavior, browser throttling, and WebXR availability must be measured on hardware. [Quest testing](quest-testing.md) is the test record.

## Intended connection

1. Connect the Quest and host to the same trusted Wi-Fi/LAN. A 5 GHz or 6 GHz access point near the Quest is a useful starting condition, not a measured requirement.
2. Run the host and read its actual HTTPS URL and pairing code from the host screen or terminal.
3. Enter the exact URL in Quest Browser. The terminal also prints a QR code, but Quest's QR-opening behavior has not been tested. A private IP is more dependable than an unverified `.local` name.
4. Check the host address in the warning. If the browser exposes certificate details, compare its SHA-256 fingerprint with the one printed by the host. Quest Browser's certificate-detail UI has not been verified, so a trusted local CA installation may be needed for a stronger identity check. See [networking](networking.md).
5. Pair using the short-lived code shown on the host. The host may then raise its screen-capture picker; approve a real display locally. Press play if browser autoplay is blocked. Use fullscreen if the browser offers it.

The current repository does not establish that this full flow works. Do not enter a pairing code or accept a certificate exception for an unexpected host.

Audio is disabled by default. If the host starts with `--audio` and native capture succeeds, an Opus audio track is offered; use the browser audio button to enable sound. No Quest audio playback, decoder, or A/V sync result has been recorded.

## Browser capabilities

The browser must negotiate H.264 through WebRTC. Resolution and frame rate depend on the host encoder, Wi-Fi, and Quest decoder; no maximum is asserted here. The client offers Performance (up to 1280×720), Balanced (up to 1920×1080), and Quality (native capture size), plus FPS and bitrate choices. The host validates FPS from 15 to 120 and bitrate from 2 to 80 Mbps. These controls have not been retested against Quest Browser. A normal `<video>` view is the initial client target. The page also contains experimental view-only WebXR code: if the browser exposes a secure-context WebXR session and WebGL2, it can enter immersive AR or VR and place the video on a flat or curved screen. Recenter, distance, and scale controls are included. None of this has been tested on Quest 3. Controller rays cannot control the host desktop.
