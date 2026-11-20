# PRX Voice Client

<!-- Badges -->
[![CI](https://github.com/openprx/prx-voice-client/actions/workflows/ci.yml/badge.svg)](https://github.com/openprx/prx-voice-client/actions)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)

An open-source, cross-platform, real-time voice AI client.

> Not a traditional IM, not a phone system, not a voice player -- a **Session-centric AI Native Voice Client**.

<!-- TODO: Add screenshot or demo GIF here -->
<!-- ![Screenshot](docs/assets/screenshot.png) -->

## Core Concept

Users can wake AI at any time and engage in natural, multi-turn, interruptible real-time voice conversations across desktop and mobile platforms.

## Quick Start

```bash
# Clone the repository
git clone https://github.com/openprx/prx-voice-client.git
cd prx-voice-client

# Build and run the desktop app
cargo run -p prxvc-desktop

# Or build a release binary
cargo build -p prxvc-desktop --release
./target/release/prxvc-desktop
```

See the [Installation Guide](docs/installation.md) for platform-specific prerequisites.

## Platform Matrix

| Platform | Tech Stack | V1 Role | Priority |
|----------|-----------|---------|----------|
| Desktop | Rust + egui/eframe | Persistent work assistant | P0 |
| iOS | Swift/SwiftUI + Rust Core | Lightweight native voice entry | P1 |
| Android | Kotlin/Compose + Rust Core | Lightweight native voice entry | P1 |

## Project Structure

```
prx-voice-client/
├── apps/
│   ├── desktop/          # Rust + egui desktop application
│   ├── ios/              # Native iOS app
│   └── android/          # Native Android app
├── crates/
│   ├── core/             # App-level shared entry, lifecycle, engine
│   ├── session/          # Session + interpreting state machines, turns, driver
│   ├── audio/            # Audio I/O, per-app capture, resampling, VAD, buffers
│   ├── protocol/         # Command/event contracts, transport
│   ├── config/           # Configuration model, defaults, migration
│   ├── storage/          # Local SQLite storage
│   ├── observe/          # Tracing, metrics, diagnostics
│   ├── security/         # Token/secret management, sanitization
│   ├── ffi/              # C API bridge for iOS/Android
│   ├── desktop-ui/       # egui view layer
│   ├── i18n/             # Internationalization (EN, ZH-CN)
│   ├── adapters/         # MT/TTS adapters (translation, voice synthesis)
│   └── testkit/          # Test utilities and mocks
├── docs/                 # Documentation
└── scripts/              # Build and CI scripts
```

## Documentation

| Document | Description |
|----------|-------------|
| [Installation Guide](docs/installation.md) | Build and install on all platforms |
| [User Guide](docs/user-guide.md) | Features, settings, keyboard shortcuts, troubleshooting |
| [Architecture Overview](docs/architecture.md) | System design, crate map, data flow, threading model |
| [Protocol Specification](docs/protocol.md) | Commands, events, error codes, transport details |
| [CHANGELOG](CHANGELOG.md) | Release history and changes |
| [CONTRIBUTING](CONTRIBUTING.md) | How to contribute |

## Build

```bash
# Build entire workspace
cargo build --workspace

# Check compilation
cargo check --workspace

# Run tests
cargo test --workspace

# Run desktop app
cargo run -p prxvc-desktop

# Lint
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Engineering Standards

- Rust 2024 Edition (1.85+)
- Zero `unwrap()` / `expect()` / `todo!()` / `unimplemented!()` in production code
- Explicit error handling with `thiserror`
- Audio thread and UI thread strictly isolated
- All state changes through the session state machine
- Structured tracing with sensitive data sanitization
- `parking_lot::Mutex` for synchronization (not `std::sync::Mutex`)

## License

[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE). The bundled fonts (Hanken Grotesk, JetBrains Mono, Noto Sans SC) are licensed under the SIL Open Font License 1.1; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
