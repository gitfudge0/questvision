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
make run ARGS='--audio --port 48000'   # Start the optimized host (app on macOS)
make dev                              # Start the debug host
make doctor                           # Run diagnostics
make benchmark PRESET=performance      # Capture and encode real frames
make verify                           # Check formatting, lint, test, build release
```

`ARGS` passes extra arguments to the command, `PRESET` defaults to `balanced`, and `CARGO` can override the Cargo executable. `make verify` runs each check in order and stops on the first failure, even with `make -j verify`.

On macOS, `make app` creates `target/release/Quest Display.app` with a copied release host and local ad-hoc signature. Set `CODE_SIGN_IDENTITY` to use an available signing identity. `make run` delegates to `make app-run`, which launches the bundle through LaunchServices and keeps pairing output in the interactive terminal. Ctrl-C stops the app launched by that command. `make dev` runs the debug host directly; on other platforms, `make run` runs the release host directly. See [macOS](macos.md) for signing and permission limitations.

## Adding a backend

Expose backend name, source identity, size, pixel format, timestamp, memory location, and whether a copy occurred. Keep the queue bounded and record dropped frames. Keep permissions inside the platform adapter; the WebRTC and HTTP layers should not know platform-specific handles. A source switch should release the previous capture before opening the next source when required by the OS API.

Native encoders should report actual hardware selection, codec profile, latency settings, bitrate, and keyframe requests. Do not present OpenH264 as hardware accelerated. For audio, keep timestamps on the same monotonic timebase as video and measure drift.

## Review requirements

Use `cargo fmt` and scoped tests while coding. Before a release, audit the Cargo lockfile licenses and advisories, H.264 distribution obligations, TLS defaults, pairing and Origin checks, LAN binding, and logs. Test a fresh install on each advertised OS. Update [implementation status](implementation-status.md) and [Quest testing](quest-testing.md) with commands and observed results. No test result should be copied from another machine without its environment details.
