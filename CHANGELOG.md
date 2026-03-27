# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-03-27

### Added
- Initial project structure with 13 Rust crates
- Session state machine (14 states, 24 valid transitions)
- Command/event bus (tokio mpsc + broadcast)
- Mock Gateway for development and testing
- WebSocket transport for real network communication
- Audio pipeline with cpal integration (device enumeration, capture, playback)
- Desktop app (Rust + egui) with 5 pages (Session, History, Settings, Debug, About)
- Global hotkey support (Ctrl+Shift+Space)
- System tray with menu controls
- Local storage (SQLite) for session history
- TOML configuration with platform-aware paths
- Internationalization (English + Chinese)
- FFI layer with C API for iOS and Android
- iOS project skeleton (SwiftUI + Rust bridge)
- Android project skeleton (Kotlin Compose + JNI bridge)
- Observability (structured tracing, atomic metrics, diagnostics export)
- Security abstractions (credential store trait, data sanitization)
- Auto-reconnection with exponential backoff
- Error display with recovery actions
- Network status indicator
- CI/CD (GitHub Actions for test, build, release)
- 151+ unit and integration tests
- Open source governance documentation
