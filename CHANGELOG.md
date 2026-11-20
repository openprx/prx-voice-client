# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Bilingual consecutive-interpreting state machine and turn driver (`prxvc-session`)
- macOS per-application audio capture via Core Audio process taps, 16 kHz mono
  resampling, and audio-routing traits (`AppAudioSource`, `VirtualMicSink`) with a
  BlackHole virtual-mic backend (`prxvc-audio`)
- `prxvc-adapters` crate: machine-translation and speech-synthesis adapters,
  including an OpenAI-compatible translator and a development tone synthesizer
- RF design-standard theme (`prxvc-desktop-ui::theme`): colour, type, radius and
  spacing tokens for light and dark, applied to egui globally; it is the default
  theme and follows the system light/dark preference. The previous colour themes
  stay selectable. Settings, history and debug dialogs are rebuilt on it with new
  RF-styled widgets (switch, segmented control, select, buttons, text field)
- Bundled UI fonts per the design standard: Hanken Grotesk (400, 500, 600),
  JetBrains Mono (400, 600) and a GB2312 subset of Noto Sans SC (400, 600), as
  named egui font families wired into the theme's text styles. The system CJK
  font stays as a fallback. Licences in `THIRD_PARTY_NOTICES.md`;
  `scripts/build-fonts.sh` reproduces the font files

### Changed
- Settings dialog regrouped into sections of one-setting rows (label and
  description left, control right); section titles are now sentence case

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
