# Linux

## Current state

Linux capture uses `scrcap`, which opens an xdg-desktop-portal ScreenCast session and receives frames through PipeWire. The portal owns source consent. This route respects Wayland's capture model. A real Hyprland desktop reached Chromium through WebRTC in one local test; see [status](implementation-status.md). Other compositors and a separate LAN client remain unverified.

## Session requirements

- Run inside a logged-in graphical desktop session, not a headless SSH shell.
- Keep the user's D-Bus session, PipeWire, and a compatible xdg-desktop-portal backend running.
- On Wayland, approve the portal source picker. A compositor may choose whether it offers a whole monitor, a window, or a virtual output.
- Use the host and Quest on the same trusted LAN. The portal does not grant network access or configure a firewall.

The [portal ScreenCast API](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html) creates a session, selects sources, starts with user consent, and returns a PipeWire remote. `scrcap` documents this route for Linux. The first automated Hyprland attempt reported D-Bus `Did not receive a reply` while the xdg-desktop-portal-wlr monitor picker remained pending. After the test selected a monitor, the real desktop streamed to Chromium. A normal user must complete the picker on the host. GNOME, KDE Plasma, and Sway Wayland sessions still need capture tests. There is no X11-specific backend or verified X11 portal route.

## Build and run

Install the development packages listed in [building](building.md), then:

```sh
cargo build --release
target/release/questdisplay --help
```

Run `target/release/questdisplay start`, open its printed URL, pair, and choose a monitor in the host portal picker. Start with Performance. After tuning, the final same-host Chromium UI showed about 43 fps at 1152×720 Performance and 23 fps at 1728×1080 Balanced. These were instantaneous readings, below the 60 fps goal; the 120-frame host benchmark measured 41.1 and 20.7 fps respectively. A quality switch raised the portal picker again. There is no AppImage, `.deb`, `.rpm`, Arch package, or systemd user unit yet.

Desktop audio is optional: run `target/release/questdisplay start --audio` and enable sound in the browser after an audio track arrives. The native `scrcap` audio source is encoded as bundled Opus at 48 kHz stereo in 20 ms packets; unsupported capture falls back to video only. In a live Linux test after unlocking the desktop and completing the portal picker, same-host Chromium received live audio and video tracks. A two-second 440 Hz tone played through PipeWire produced a 0.018585 peak in a WebAudio analyser on the received track, falling to zero after 1.5 seconds of silence. The browser audio button enabled and muted playback. Human listening, separate LAN playback, and A/V synchronization were not tested. Sources with more than two channels use their first left and right channels.

## Virtual monitor

`questdisplay virtual status`, `virtual create`, and `virtual list` manage an optional headless output on Hyprland. One live Hyprland test created and removed a 1920×1080@60 `QUESTDISPLAY-*` output. The create command stays open until Ctrl-C or SIGTERM so it can remove the output it owns. Sway code exists but has no live test. The browser cannot explicitly select a virtual output. This host's portal did not advertise its VIRTUAL source type, and capture of the headless output through the picker was not confirmed. See [virtual displays](virtual-displays.md). Physical-display capture works independently.
