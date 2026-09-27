# Build from source

The host is a Rust 2024 edition project. Install stable Rust using the [official rustup instructions](https://rust-lang.org/tools/install/), a C/C++ compiler for bundled OpenH264, CMake for bundled Opus, and the platform SDK. CMake is a **source-build prerequisite only**. A user running a built binary does not need to install CMake, Opus, Node.js, FFmpeg, GStreamer, or Python.

```sh
git clone https://github.com/gitfudge0/questvision.git
cd questvision
cargo build --release
cargo test
target/release/questdisplay --help
target/release/questdisplay start
```

Run these commands from a local clone or source checkout. The binary name is `questdisplay`. The CLI also exposes `doctor`, `displays`, `config`, `devices`, `devices revoke <id>`, `devices revoke-all`, and `benchmark --preset performance|balanced|quality`. The benchmark captures and encodes 120 real frames after native source permission; it reports output size, measured frame rate, mean resize/color-conversion/H.264 encode time, and encoded bitrate. It does not measure capture API time or end-to-end latency. `displays` reports the system-selected source, `devices` lists digest-derived IDs, and `config` redacts stored credential digests. A successful build is only a build result, not proof of streaming.

## Linux

Linux builds need the PipeWire, SPA, and D-Bus development headers and `pkg-config`; `scrcap` also needs Clang/libclang for bindings. On Debian or Ubuntu, after [installing Rust](https://rust-lang.org/tools/install/), run:

```sh
sudo apt update
sudo apt install build-essential clang libclang-dev cmake pkg-config libpipewire-0.3-dev libspa-0.2-dev libdbus-1-dev
cargo build --release
```

Package names differ on Fedora and Arch. Install the corresponding development packages for `libpipewire-0.3`, SPA, D-Bus, Clang, and CMake, then run `cargo build --release`. The built Linux binary currently links normal desktop libraries and is not a fully static standalone executable. Opus is bundled; the inspected local release binary had no dynamic libopus dependency. At runtime, a Wayland session needs a working PipeWire service and an xdg-desktop-portal backend supplied by the desktop. See [Linux](linux.md).

An optimized Linux x86_64 build succeeded on the development host and dynamically linked `libpipewire-0.3` and `libdbus`. This is a local build result, not a fresh-machine installation test.

The final local build also produced `dist/questdisplay-linux-x86_64.tar.gz` with the unsigned binary, README, LICENSE, and docs. This archive has not been tested on a fresh machine and is not a published installer.

## macOS

Install Rust through [rustup](https://rust-lang.org/tools/install/), Xcode Command Line Tools, and [CMake](https://cmake.org/download/) on the build machine, then build:

```sh
xcode-select --install
cargo build --release
```

This produces an unsigned command-line binary. There is no notarized `.app` or `.dmg` yet. Screen Recording permission behavior must be verified with an actual Mac build.

## Windows

Install the x64 Windows [rustup installer](https://rust-lang.org/tools/install/), the Visual Studio C++ Build Tools with the Windows SDK, and [CMake](https://cmake.org/download/) on the build machine. Choose the Rust MSVC toolchain and put CMake on `PATH`. In a new PowerShell terminal:

```powershell
cargo build --release
cargo test
.\target\release\questdisplay.exe --help
```

This produces an unsigned executable. No installer or virtual display driver is included.

## Cross-compilation and release packages

A tag matching `v*` triggers the [release archive workflow](../.github/workflows/release.yml). It is configured to upload `questdisplay-linux-x86_64.tar.gz`, `QuestDisplay-macos-arm64.tar.gz`, `QuestDisplay-macos-x86_64.tar.gz`, and `QuestDisplay-windows-x86_64.zip` as Actions artifacts. No tag build has been observed yet, so availability and cross-platform compilation remain unverified. Linux aarch64 is not configured. These archives contain unsigned CLI binaries, not installers or `.app` packages. The Linux binary dynamically links PipeWire, SPA, D-Bus, and other normal system libraries; its target machine needs those facilities and a graphical portal session. See [implementation status](implementation-status.md).
