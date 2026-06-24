# Installation Guide

This guide covers building and installing PRX Voice Client on all supported platforms.

## Desktop

### Prerequisites

- **Rust toolchain** 1.85 or newer (stable channel)
- **Cargo** (included with Rust)

Install Rust via [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup update stable
```

Verify your version:

```bash
rustc --version  # must be >= 1.85
```

### macOS

macOS includes CoreAudio by default. No additional audio libraries are required.

If you need development tools:

```bash
xcode-select --install
```

Build the desktop application:

```bash
cargo build -p prxvc-desktop --release
```

The binary will be at `target/release/prxvc-desktop`. Run it directly:

```bash
./target/release/prxvc-desktop
```

Or run in development mode:

```bash
cargo run -p prxvc-desktop
```

### Linux

Install audio development libraries for ALSA (required by `cpal`):

**Debian / Ubuntu:**

```bash
sudo apt update
sudo apt install -y libasound2-dev pkg-config build-essential
```

**Fedora / RHEL:**

```bash
sudo dnf install -y alsa-lib-devel pkg-config
```

**Arch Linux:**

```bash
sudo pacman -S alsa-lib pkg-config
```

Build the desktop application:

```bash
cargo build -p prxvc-desktop --release
```

The binary will be at `target/release/prxvc-desktop`.

### Windows

Install the Rust toolchain using [rustup-init.exe](https://win.rustup.rs/). Windows audio (WASAPI) is supported by `cpal` out of the box with no extra dependencies.

Ensure you have the [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) installed (the "Desktop development with C++" workload).

Build the desktop application:

```bash
cargo build -p prxvc-desktop --release
```

The binary will be at `target\release\prxvc-desktop.exe`.

### Build from Source (All Platforms)

```bash
# Clone the repository
git clone https://github.com/openprx/prx-voice-client.git
cd prx-voice-client

# Build the entire workspace
cargo build --workspace

# Build only the desktop binary (release mode)
cargo build -p prxvc-desktop --release

# Run all tests
cargo test --workspace

# Run the desktop app (debug mode)
cargo run -p prxvc-desktop
```

### Release Profiles

| Profile | Optimized For | Command |
|---------|--------------|---------|
| `dev` | Fast compilation, debug symbols | `cargo build` |
| `release` | Binary size (LTO + strip) | `cargo build --release` |

### Pre-built Binaries

Pre-built binaries for macOS, Linux, and Windows are available on the
[GitHub Releases](https://github.com/openprx/prx-voice-client/releases) page.

Download the appropriate archive for your platform, extract it, and run the binary.

## iOS

### Prerequisites

- macOS with **Xcode 15+**
- Rust toolchain with iOS targets:

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
```

### Setup

1. Build the Rust static library for iOS:

```bash
cd apps/ios
./build-rust.sh
```

2. Open the Xcode project:

```bash
open PRXVoiceClient.xcodeproj
```

3. Select a target device or simulator and build from Xcode.

For detailed iOS-specific setup, see [apps/ios/README.md](../apps/ios/README.md).

## Android

### Prerequisites

- **Android Studio** (latest stable)
- **Android NDK** (installed via SDK Manager)
- **cargo-ndk** for cross-compiling Rust to Android targets:

```bash
cargo install cargo-ndk
```

- Rust Android targets:

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android
```

### Setup

1. Set the `ANDROID_NDK_HOME` environment variable:

```bash
export ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/<version>
```

2. Build the Rust shared library for Android:

```bash
cd apps/android
./build-rust.sh
```

3. Open the `apps/android/app` project in Android Studio and build normally.

For detailed Android-specific setup, see [apps/android/README.md](../apps/android/README.md).

## Verifying Your Installation

After building, run the test suite to confirm everything is working:

```bash
cargo test --workspace
```

Check compilation without running tests:

```bash
cargo check --workspace
```

Run the linter:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```
