# Troubleshooting

Start with `questdisplay doctor` and the host log. Doctor currently reports OS, listen address, session type, and basic capture guidance; it does not probe the hardware encoder, firewall, audio, or certificate. Keep the host and browser on the same trusted network.

| Symptom | Check |
| --- | --- |
| The page will not open | Use the exact IP and port printed by the host, not an example address. Check that the process is listening, the host firewall allows the private-network TCP port, and the Quest is not on an isolated guest Wi-Fi network. |
| Browser shows a certificate warning | A first-run self-signed certificate is expected. Check the host IP and certificate fingerprint locally before accepting it. Do not turn off certificate checks for the browser. |
| Pairing returns HTTP 503 | Run the host in an interactive terminal. Headless pairing is disabled because the host code must not be written to service logs. |
| A paired browser should lose access | Run `questdisplay devices` locally, then `questdisplay devices revoke <id>` or `questdisplay devices revoke-all`. A live Chromium smoke confirmed a prior token then received 401 and the browser returned to pairing. An already-playing video's interruption timing was not measured. |
| Page opens but video does not start | Check pairing and WebRTC connection state. Browser autoplay may require a tap. Check UDP firewall rules, Wi-Fi isolation, and whether H.264 was negotiated. A working HTTPS page does not prove ICE connectivity. |
| Capture was denied but pairing still works | The host now returns HTTP 422 for a capture failure, leaving the paired browser credential intact. Retry the capture permission or source picker. This classification fix has not yet had a post-fix live denial test. |
| Linux picker does not appear | Run in the graphical user session. Check `systemctl --user status pipewire xdg-desktop-portal` and the compositor's portal backend. The portal may not offer the requested source type. |
| Linux portal returns `Did not receive a reply` | In the first Hyprland test, the portal's monitor picker was still waiting for a selection. Look for the host-side picker and choose a monitor. If it still fails, check `journalctl --user -u xdg-desktop-portal -u xdg-desktop-portal-wlr` and the D-Bus session. A later run succeeded after picker selection. |
| macOS shows a black frame or permission error | Check System Settings, Privacy & Security, Screen Recording. Grant access to the actual app or terminal that launched the binary, then restart it. |
| Windows picker does not appear | Run from an interactive desktop session. Check Windows capture support and private-network firewall permissions. |
| High delay or low frame rate | Record host encode time and queue depth, browser dropped/decoded frames, packet loss, and RTT. Lower resolution or frame rate. Do not assume Wi-Fi is the sole cause when using software encoding. |
| Virtual monitor absent | On Hyprland, check `questdisplay virtual status` and start `questdisplay virtual create` in another terminal. Sway support has code but no live test. Other compositors and Windows/macOS virtual display creation are unavailable here. Capture of a created headless output through the portal is unverified. |
| Audio absent | Audio is off by default. Start the host with `--audio`, then enable sound in the browser when its button appears. If native audio capture is unsupported, the host continues with video only and `/api/status.audio` reports false. Live Linux audio reached same-host Chromium; audible output and Quest playback remain unverified. |
| Remote input absent | Input injection is not implemented; the stream is view-only. |

If a build fails, use the OS-specific prerequisites in [building](building.md). On Linux, `pkg-config` errors usually name the missing PipeWire, SPA, or D-Bus development package. Keep the raw error and host OS version when filing an issue. Never include a pairing code, token, certificate key, or desktop screenshot that contains private information in a public issue.
