# Development

The host is a single Rust package. Keep native capture, encoding, WebRTC transport, HTTP/signaling, authorization, and browser UI behind separate interfaces even if they begin as modules in one crate. A new OS backend should return explicit availability and permission errors; it must not quietly replace real capture with generated frames.

## Local checks

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Run a real capture test on each OS after the build. CI without a logged-in graphical session cannot validate portal consent, ScreenCaptureKit permission, Windows Graphics Capture, hardware encoding, or Quest playback. Keep the browser test on a synthetic source separate from platform capture tests and label synthetic video as such.

## Make shortcuts

The optional Makefile requires GNU Make, including the version shipped with macOS. Rust and native build prerequisites remain as described in [building from source](building.md). Windows users can keep using the documented Cargo commands.

```sh
make                                  # Show all targets
make release                          # Build the optimized binary
make app                              # Build a local macOS app bundle
make run                              # Open the dashboard on macOS; start the host elsewhere
make gui                              # Open the optimized GPUI dashboard
make host-run ARGS='--audio --port 48000' # Start the optimized CLI host
make app-run                          # Open the signed macOS GPUI dashboard
make app-host-run                     # Start the signed macOS CLI host
make gui-dev                          # Open the debug GPUI dashboard directly
make dev                              # Start the debug host
make doctor                           # Run diagnostics
make benchmark PRESET=performance      # Capture and encode real frames
make verify                           # Check formatting, lint, test, build release
```

`ARGS` passes extra arguments to the command, `PRESET` defaults to `balanced`, and `CARGO` can override the Cargo executable. `make verify` runs each check in order and stops on the first failure, even with `make -j verify`.

On macOS, `make app` creates `target/release/Quest Display.app` with a copied release executable and local ad-hoc signature. Set `CODE_SIGN_IDENTITY` to use an available signing identity. `make run`, `make gui`, and `make app-run` open the GPUI dashboard from this signed bundle through LaunchServices so macOS can identify Quest Display when screen recording permission is requested. Permission is requested when capture starts. `make host-run` and `make app-host-run` launch the packaged CLI host with the `start` command.

The macOS app launcher requires an interactive terminal to relay output and handle Ctrl-C, which stops the app launched by that command. `make gui-dev` opens the debug dashboard directly, and `make dev` starts the debug CLI host directly. macOS can attribute permissions for these direct command-line launches to Terminal; use the packaged GUI targets when granting permission to Quest Display. On other platforms, `make run` and `make host-run` start the release CLI host, and `make gui` opens the release dashboard directly. See [macOS](macos.md) for signing and permission limitations.

## Adding a backend

Expose backend name, source identity, size, pixel format, timestamp, memory location, and whether a copy occurred. Keep the queue bounded and record dropped frames. Keep permissions inside the platform adapter; the WebRTC and HTTP layers should not know platform-specific handles. A source switch should release the previous capture before opening the next source when required by the OS API.

Native encoders should report actual hardware selection, codec profile, latency settings, bitrate, and keyframe requests. Do not present OpenH264 as hardware accelerated. For audio, keep timestamps on the same monotonic timebase as video and measure drift.

## Review requirements

Use `cargo fmt` and scoped tests while coding. Before a release, audit the Cargo lockfile licenses and advisories, H.264 distribution obligations, TLS defaults, pairing and Origin checks, LAN binding, and logs. Test a fresh install on each advertised OS. Update [implementation status](implementation-status.md) and [Quest testing](quest-testing.md) with commands and observed results. No test result should be copied from another machine without its environment details.
