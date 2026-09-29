# macOS

## Current state

The selected capture wrapper uses [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit). Apple's API sends display or window frames through a permission-controlled stream. Quest Display does not yet have a verified macOS capture-to-browser run. A local Mac release build is available, but audio playback remains unverified. Optional `--audio` code requests native audio through `scrcap` and uses bundled Opus. Hardware VideoToolbox encoding, native menu-bar UI, Developer ID signed distribution, and notarized `.dmg` are not implemented.

Apple Silicon is the first physical test target. The release workflow also attempts an Intel build, but neither architecture has a capture test yet. A minimum supported macOS version has not been established by this project.

## Build and permission flow

Install Rust, Xcode Command Line Tools, and the build prerequisites below, then run `make run` from an interactive terminal. On macOS this builds the local app bundle and opens its GPUI dashboard through LaunchServices, with logs connected to that terminal. In Overview, click **Request permission** if Screen Recording is not granted, then enable Quest Display in System Settings > Privacy & Security > Screen Recording. Apple's [ScreenCaptureKit sample](https://developer.apple.com/documentation/screencapturekit/capturing-screen-content-in-macos) notes that a restart can be required after first granting permission. Once access is granted, click **Start host**, open the displayed connection URL in Quest Browser, and request pairing; the dashboard shows the pairing code. Choose a display if the system picker appears. Keep the application open while streaming. Quit the dashboard or press Ctrl-C in the launching terminal to stop that app instance. Dismiss any open capture picker if stopping waits for it. The launcher refuses to start if this bundle identifier is already running.

Install CMake on the build machine for the bundled Opus library; it is not needed to run the binary. Enable **Desktop audio** in dashboard Settings and save before starting the host, or add `--audio` to a CLI start command, then explicitly enable sound in the browser. Unsupported audio capture falls back to video only. A/V synchronization is unmeasured.

`cargo build --release` still produces an unsigned CLI executable, and `target/release/questdisplay start` can run it directly for development. macOS may identify the permission owner as the launching terminal in that case. `make dev` also runs the debug host directly. Do not infer that macOS capture works from a successful build.

## Local app bundle

On macOS, run `make app` to build the release host and package `target/release/Quest Display.app`. The bundle contains the actual Rust executable and GPUI dashboard, uses the identifier `io.github.gitfudge0.questdisplay`, and includes screen and desktop audio capture usage descriptions. Packaging checks the metadata and code signature. It adds no sandbox, screen capture entitlements, or icon.

New bundles default to local ad-hoc signing. Set `CODE_SIGN_IDENTITY` to an available signing identity when packaging, for example `CODE_SIGN_IDENTITY='Your signing identity' make app`. When this variable is unset, packaging preserves the existing output bundle's certificate signer by matching its certificate fingerprint to a valid keychain identity. If that identity is unavailable, packaging stops before changing the bundle. An explicit `CODE_SIGN_IDENTITY` overrides this choice; use `-` to request ad-hoc signing. Existing ad-hoc bundles retain ad-hoc signing, and ad-hoc rebuilds may require granting Screen Recording permission again. Distribution needs a trusted, stable signing identity; this local bundle is not a Developer ID signed or notarized release.

`make app-run` (also used by `make run`) opens the bundled GPUI dashboard. Its Overview displays the connection URL, QR code, and pairing code, so GUI pairing does not require reading terminal output. The bundle's permission attribution and macOS streaming still need a physical capture-to-browser test.

## Dashboard permissions

The GPUI Overview shows the current Screen Recording permission for the running app. Opening the dashboard checks access without displaying a permission prompt. Click **Request permission** to ask macOS for access, then enable **Quest Display** in System Settings > Privacy & Security > Screen Recording (called Screen & System Audio Recording on some macOS versions). The dashboard refreshes the status periodically and immediately after a request. If macOS asks for a restart, or capture still fails after granting access, quit and reopen Quest Display.

The permission request must come from the packaged app for macOS to attribute it to Quest Display. The bundle keeps the identifier `io.github.gitfudge0.questdisplay` and declares usage descriptions for screen capture and optional desktop audio capture. The Screen Recording status does not verify desktop audio playback or a successful stream.

If Quest Display is enabled in Screen & System Audio Recording but the dashboard still reports access denied after the signing identity changes, its saved permission entry may refer to the previous signature. Quit Quest Display, remove its entry from System Settings > Privacy & Security > Screen & System Audio Recording, then add `/Applications/Quest Display.app`, enable access, and relaunch the installed app. Preserving the certificate signer on later rebuilds avoids reverting that identity.

Local ad-hoc signing lets the app run, but a rebuilt binary can require granting permission again. For consistent identity across local rebuilds, use a stable Apple Development signing identity with `CODE_SIGN_IDENTITY`. A Developer ID Application identity is intended for distribution outside the App Store. Both require an identity and its private key in the signing machine's keychain; packaging cannot create an Apple developer identity.

## Virtual monitor

Quest Display cannot create a macOS virtual extended desktop today. ScreenCaptureKit captures existing content. We have not identified a supported public API for an ordinary app to add an arbitrary host display. Apple's Paravirtualized Graphics APIs apply to a virtualization context, so they do not establish such a host-app feature. See [virtual displays](virtual-displays.md).
