# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- Recognised units that are only filler ("嗯", "Mm.", "Uh") are no longer translated or spoken
  (`pipeline.drop_filler`, default on); they are counted and traced. A trailing "嗯。" after real
  speech was spoken in 10 of 15 Mandarin utterances of the benchmark set.
- An empty refine result keeps the streaming text and no longer counts towards the
  `RefineFailing` degradation.
- Text with nothing to say aloud (punctuation only) is never sent to the synthesiser, and in the
  live call a short clause is held until it ends inside its generation cap and retried once, so a
  synthesis runaway is not played.
- The call coordinator's log line names the real previous state.
- A second click on the orb while the assistant listens ends the session.

### Added
- The Call translation window (menu-bar menu and the orb's menu): state, Start and Stop, Pause and
  Resume, the languages, issues with their actions, live captions of both directions. The Debug
  window keeps the raw diagnostics and a link to it.
- "End assistant and start": ends the assistant session that blocks call mode and starts the call
  with the same options once the microphone is free.
- `pipeline.min_speech_dbfs` (default -75): a start of speech below this level is not decoded.

### Changed
- `prxvc-audio` capture and playback no longer lock or allocate on the device
  callbacks: the microphone callback writes into a preallocated lock-free ring and
  the output callback drains a lock-free playback ring. Both endpoints implement
  the `AudioSource` and `AudioSink` ports with `prxvc-media` frames; the old
  `AudioInput`, `AudioOutput`, `AppAudioSource` and `VirtualMicSink` traits are gone.
  A test fails when real-time code (`*_rt.rs`) uses a lock, an allocation, logging or
  a channel send.
- The sink that located one third-party virtual device by name is removed; the virtual microphone
  endpoint replaces it.

### Added
- `prxvc-audio` `virtual_mic::VirtualMicSink`: writes into the PRX Voice microphone
  driver's hidden sink, opened by UID (`audio.virtual_mic_device`, empty means our
  driver), and `driver_status::DriverStatus`: whether the driver bundle is installed,
  its variant and version (read from `Info.plist`), whether the audio service has loaded
  it and whether the version is the one the app expects. Any other output device can
  be used by entering its UID.
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
  resampling, and audio-routing traits (`AppAudioSource`, `VirtualMicSink`)
  (`prxvc-audio`)
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
