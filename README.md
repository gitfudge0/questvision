# Quest Display

Quest Display is an experimental local display streamer. The host is a Rust process; the client is a web page. The intended connection is a computer and a Meta Quest on the same LAN, with video carried by WebRTC. It needs no cloud account or native Quest app.

**This repository is under active implementation. It is not yet a verified install-and-stream release for macOS, Windows, or Quest 3.** See [implementation status](docs/implementation-status.md) before trying it. Hardware encoding and remote input are not implemented. Optional Linux desktop audio reached same-host Chromium in a live test; human listening and Quest playback were not tested. Hyprland virtual-output management works on the tested host, but the browser cannot explicitly select that output. Experimental view-only WebXR code exists but has not been tested on a Quest. No Quest device test or glass-to-glass latency benchmark has been recorded.

## Quick start from source

```sh
git clone https://github.com/gitfudge0/questvision.git
cd questvision
```

1. Install a current Rust toolchain and the platform build prerequisites in [building](docs/building.md).
2. Run `cargo build --release`.
3. Run `target/release/questdisplay start` on Linux/macOS or `.\target\release\questdisplay.exe start` in Windows PowerShell. The host listens on the detected private LAN IP at port 47990. Use `--listen <private-ip> --port <port>` if detection selects the wrong interface.
4. Keep the terminal open to see the listening address, QR code, and certificate fingerprint. Put the Quest and host on the same trusted LAN.
5. In Quest Browser, open the printed `https://<host-ip>:<port>` URL. A QR code is also printed for devices with a scanner. Check the host address before accepting its certificate warning; compare the SHA-256 fingerprint if the browser exposes certificate details.
6. Pair using the six-digit code that appears in the host terminal. When streaming starts, approve the operating system's screen capture prompt on the host. On Linux, the portal may ask which monitor to share. Use the browser fullscreen control where available.

Audio is off by default. To request desktop audio, start the host with `questdisplay start --audio` (or `target/release/questdisplay start --audio` from a source build), then use the browser's audio button to enable sound after the track arrives. The host falls back to video only if native audio capture is unavailable. A test tone reached same-host Chromium through Opus; audible speaker output, Quest playback, and A/V synchronization remain unverified.

The HTTPS page, pairing, Linux Wayland capture, and WebRTC playback worked in Chromium on the development host. After tuning, a 120-frame host benchmark measured 41.1 fps at 1152×720 Performance and 20.7 fps at 1728×1080 Balanced. In the final live browser smoke, the UI showed about 43 and 23 fps respectively. Neither mode has met the 60 fps goal. Quest 3, macOS, Windows, and fresh-machine setup remain unverified. There are no installers yet. The binary is not signed or notarized.

## How it is meant to work

```text
OS capture API -> H.264 encoder -> WebRTC video track -> Quest Browser
                                            ^
                          embedded HTTPS server and signaling
```

Linux uses the desktop portal and PipeWire route offered by `scrcap`. macOS uses ScreenCaptureKit. Windows uses Windows Graphics Capture. The initial video encoder is bundled OpenH264 software encoding. Optional native desktop audio uses bundled Opus. Native hardware video encoder paths remain separate work. See [architecture](docs/architecture.md) and [decisions](docs/architecture-decisions.md).

## Platform guides

- [Linux](docs/linux.md)
- [macOS](docs/macos.md)
- [Windows](docs/windows.md)
- [Quest Browser](docs/quest.md)
- [Network and TLS](docs/networking.md)
- [Security](docs/security.md)
- [Troubleshooting](docs/troubleshooting.md)

## Build and contribute

[Build instructions](docs/building.md), [development notes](docs/development.md), [performance methodology](docs/performance.md), and [Quest test log](docs/quest-testing.md) record what has and has not been measured. The project is MIT licensed. Third-party licenses and H.264 patent considerations are described in [architecture decisions](docs/architecture-decisions.md).
