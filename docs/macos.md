# macOS

## Current state

The selected capture wrapper uses [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit). Apple's API sends display or window frames through a permission-controlled stream. Quest Display does not yet have a verified macOS capture-to-browser run. A local Mac release build is available, but audio playback remains unverified. Optional `--audio` code requests native audio through `scrcap` and uses bundled Opus. Hardware VideoToolbox encoding, native menu-bar UI, Developer ID signed distribution, and notarized `.dmg` are not implemented.

Apple Silicon is the first physical test target. The release workflow also attempts an Intel build, but neither architecture has a capture test yet. A minimum supported macOS version has not been established by this project.

## Build and permission flow

Install Rust, Xcode Command Line Tools, and the build prerequisites below, then run `make run` from an interactive terminal. On macOS this builds the local app bundle and launches it through LaunchServices with input and output connected to that terminal. Choose a display if the system picker appears. Grant Screen Recording in System Settings, Privacy & Security. Apple's [ScreenCaptureKit sample](https://developer.apple.com/documentation/screencapturekit/capturing-screen-content-in-macos) notes that a restart can be required after first granting permission. Keep the application open while connecting from Quest Browser; press Ctrl-C in the launching terminal to stop that app instance. The launcher refuses to start if this bundle identifier is already running.

Install CMake on the build machine for the bundled Opus library; it is not needed to run the binary. Add `--audio` to the start command to request desktop audio, then explicitly enable sound in the browser. Unsupported audio capture falls back to video only. A/V synchronization is unmeasured.

`cargo build --release` still produces an unsigned CLI executable, and `target/release/questdisplay start` can run it directly for development. macOS may identify the permission owner as the launching terminal in that case. `make dev` also runs the debug host directly. Do not infer that macOS capture works from a successful build.

## Local app bundle

On macOS, run `make app` to build the release host and package `target/release/Quest Display.app`. The bundle contains the actual Rust executable, uses the identifier `io.github.gitfudge0.questdisplay`, and includes a screen capture usage description. Packaging checks the metadata and code signature. It adds no sandbox, screen capture entitlements, icon, or native UI.

The default signature is local ad-hoc signing. Set `CODE_SIGN_IDENTITY` to an available signing identity when packaging, for example `CODE_SIGN_IDENTITY='Your signing identity' make app`. Ad-hoc rebuilds may require granting Screen Recording permission again. Distribution needs a trusted, stable signing identity; this local bundle is not a Developer ID signed or notarized release.

Pairing still requires an interactive terminal because the host prints its pairing code there. `make app-run` (also used by `make run`) supplies that terminal while launching the bundle. Opening this bundle in Finder does not provide a standalone pairing experience; there is no native UI to display pairing codes. The bundle's permission attribution and macOS streaming still need a physical capture-to-browser test.

## Virtual monitor

Quest Display cannot create a macOS virtual extended desktop today. ScreenCaptureKit captures existing content. We have not identified a supported public API for an ordinary app to add an arbitrary host display. Apple's Paravirtualized Graphics APIs apply to a virtualization context, so they do not establish such a host-app feature. See [virtual displays](virtual-displays.md).
