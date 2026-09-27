# Windows

## Current state

The selected capture wrapper uses [Windows Graphics Capture](https://learn.microsoft.com/en-us/windows/uwp/audio-video-camera/screen-capture). That API provides a system picker and D3D11 capture frames. The repository has not yet verified a Windows capture-to-browser run. Optional `--audio` code requests native audio through `scrcap` and uses bundled Opus, but no Windows build or audio playback test has been completed. Media Foundation hardware encode, tray UI, signed installer, and virtual monitor driver are not implemented.

Windows 11 x86_64 is the first physical test target. Windows 10 support is unverified; an API being available on some Windows 10 releases does not establish that this binary works there.

## Build and run

Install the Rust MSVC toolchain, Visual Studio C++ Build Tools, Windows SDK, and CMake on the build machine. In PowerShell:

```powershell
cargo build --release
.\target\release\questdisplay.exe --help
.\target\release\questdisplay.exe start
```

Run from an interactive desktop session. The Windows capture picker must be answered locally. Windows Defender Firewall may ask whether to allow the host on private networks; allow only the trusted private profile that contains the Quest. Do not open or forward the service port from a router. The build is currently an unsigned executable, with no Add/Remove Programs entry.

For optional sound, start with `.\target\release\questdisplay.exe start --audio` and explicitly enable sound in the browser. CMake is not needed to run the executable. Unsupported native audio capture falls back to video only; A/V synchronization is unmeasured.

## Virtual monitor

Microsoft's supported route is an [IddCx indirect display driver](https://learn.microsoft.com/en-us/windows-hardware/drivers/display/indirect-display-driver-model-overview). It is a UMDF driver that needs a Windows Driver Kit build and driver signing. This repository has no driver, installer, or uninstall path. Quest Display will not install a driver silently.
