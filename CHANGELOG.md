# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- `prxvc-audio` capture and playback no longer lock or allocate on the device
  callbacks: the microphone callback writes into a preallocated lock-free ring and
  the output callback drains a lock-free playback ring. Both endpoints implement
  the `AudioSource` and `AudioSink` ports with `prxvc-media` frames; the old
  `AudioInput`, `AudioOutput`, `AppAudioSource` and `VirtualMicSink` traits are gone.
  A test fails when real-time code (`*_rt.rs`) uses a lock, an allocation, logging or
  a channel send.

### Added
- `QwenTts` in `prxvc-adapters` (`tts::qwentts`): the in-process `qwentts.cpp` speech synthesiser behind the
  `prxvc-ports` synthesis port. A worker pool serves two requests at once through the runtime's batching, each
  request has its own bounded chunk queue and cancellation, every call carries a generation cap derived from the
  text and a runaway is aborted and reported as `Runaway`, a warm-up synthesis runs at load, prepared voices are
  cached on disk per talker model, and presets are prepared voices built from a stored reference. The raw FFI
  wrapper is behind the cargo feature `qwentts`.
- Catalogue: the TTS entry is pinned to the files `qwentts.cpp` was measured with and the 0.6B model is added as
  the speed fallback.
- macOS application bundle `PRX Voice.app` (`scripts/package-macos.sh`,
  `scripts/run-app.sh`, `packaging/macos/`): bundle identifier
  `org.openprx.prx-voice-client`, microphone and audio-capture usage strings
  (English and Simplified Chinese), icon (placeholder), ad-hoc or Developer ID
  signing, optional notarization. `prxvc-desktop --version` and `--check`.
- File logging in the application log directory when not attached to a terminal
  (size-capped, rotated); the app warns at start when it runs outside a bundle.
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
- Recognised text is no longer logged at `info` (character counts only; the text
  is logged at `debug`). The default log filter is `info` when `RUST_LOG` is unset.
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
