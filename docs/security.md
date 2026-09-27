# Security model

Treat a streamed desktop as sensitive. Video can reveal passwords, messages, and private files even when remote control is off. Use Quest Display only on a trusted LAN, keep the host address private, and stop the host when finished.

## Intended controls

- TLS protects the web connection; WebRTC encrypts media through its own DTLS/SRTP stack.
- A code shown only on the host pairs a browser before signaling starts. A paired device should receive a revocable credential.
- Viewing and remote input have separate permissions. Remote input starts disabled.
- Signaling should reject unpaired clients, foreign Origins, unexpected Host headers, expired or replayed codes, and unauthenticated WebSocket upgrades.
- Structured logs must not contain a bearer credential or desktop frame. The current CLI displays each short-lived pairing code only with an interactive host terminal.

Current code uses a six-digit code displayed on the interactive host terminal, valid for 120 seconds with five attempts and a ten-second per-IP start cooldown. Headless pairing returns HTTP 503. On success, the server stores a SHA-256 digest of the bearer token in the native config directory; the browser stores the token in local storage. The server checks the remote peer for a private or loopback IP, requires an exact Host header, and checks Origin on POST requests. Input permissions, a full threat-model test, and a security audit remain outstanding. A successful TLS handshake alone does not establish the rest of these controls.

Desktop audio is off by default. The host must start with `--audio`, and the browser must explicitly enable playback. Treat any enabled audio stream as sensitive in the same way as the screen stream.

Use `questdisplay devices` to list 16-character IDs derived from stored token digests. It never prints a bearer token. Run `questdisplay devices revoke <id>` for one entry or `questdisplay devices revoke-all` for every paired browser. The server reads the pairing store on new authenticated requests, and stream code checks it periodically while frames arrive. In a live isolated Chromium smoke, revoking an ID changed the old browser's protected API response from 200 to 401, cleared its stored token, and returned it to pairing. The server logged `stream stopped: paired device revoked`. Interrupting an already-playing video and persistence across a host restart were not measured.

Run the host interactively for first pairing. A future tray or local approval UI must replace this terminal-only prompt before packaging a background service.

## Certificate trust

A first-run self-signed certificate is not a public identity. Confirm the address and fingerprint displayed locally before accepting a browser exception. Do not disable certificate checks globally. See [networking](networking.md).

## Privacy

The current source declares no cloud account, analytics, or telemetry integration. Media is designed to stay on the LAN. Verify network behavior and dependency updates during a release audit; this is not a substitute for a packet capture or code review.
