# macOS

## Current state

The selected capture wrapper uses [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit). Apple's API sends display or window frames through a permission-controlled stream. Quest Display does not yet have a verified macOS capture-to-browser run. Optional `--audio` code requests native audio through `scrcap` and uses bundled Opus, but no Mac build or audio playback test has been completed. Hardware VideoToolbox encoding, native menu-bar UI, signed `.app`, and notarized `.dmg` are not implemented.

Apple Silicon is the first physical test target. The release workflow also attempts an Intel build, but neither architecture has a capture test yet. A minimum supported macOS version has not been established by this project.

## Build and permission flow

Install Rust and Xcode Command Line Tools, then run `cargo build --release` and `target/release/questdisplay start`. Start from an interactive terminal and choose a display if the system picker appears. Grant Screen Recording in System Settings, Privacy & Security. Apple's [ScreenCaptureKit sample](https://developer.apple.com/documentation/screencapturekit/capturing-screen-content-in-macos) notes that a restart can be required after first granting permission. Keep the application open while connecting from Quest Browser.

Install CMake on the build machine for the bundled Opus library; it is not needed to run the binary. Add `--audio` to the start command to request desktop audio, then explicitly enable sound in the browser. Unsupported audio capture falls back to video only. A/V synchronization is unmeasured.

The current build is an unsigned CLI executable. macOS may identify the permission owner as the launching terminal during development. A packaged app with a stable bundle identifier is needed for a polished permission flow. Do not infer that macOS capture works from a Linux build.

## Virtual monitor

Quest Display cannot create a macOS virtual extended desktop today. ScreenCaptureKit captures existing content. We have not identified a supported public API for an ordinary app to add an arbitrary host display. Apple's Paravirtualized Graphics APIs apply to a virtualization context, so they do not establish such a host-app feature. See [virtual displays](virtual-displays.md).
