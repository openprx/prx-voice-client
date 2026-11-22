# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Work since 0.1.0, grouped by day, newest first.

### 2026-11-22

#### Added
- Incremental re-translation (`pipeline.retranslation`, default `auto`, which turns it on for a translation engine
  without conversation context): a source prefix is committed and spoken as soon as its translation is confirmed by
  the translations of longer, later readings, so the first translated audio no longer waits for the recogniser's own
  stable prefix.
- The re-translation machinery: a per-direction draft worker that the recognition stage polls without blocking, and a
  draft cache from exact source text to its finished translation, so a committed unit is spoken exactly as verified
  and costs no second engine call.
- Sentence continuation in the translation stage: a unit that continues a sentence is translated from the start of the
  sentence and only the new part is spoken, so a context-free engine always sees whole sentences.
- Approximate alignment of the spoken prefix against a new full translation (`pipeline.retranslate_align_max_ratio`,
  default 0.5), so a slightly reworded translation still continues the sentence instead of counting as diverged.
- Nine `[pipeline]` re-translation keys with defaults; no schema change, so saved settings load unchanged.
- Re-translation figures on each utterance log line and in the call summary (figures only, no spoken text).
- `pipeline.tts_hold_max_words` (default 3) and `pipeline.tts_hold_max_chars` (default 4): only very short clauses are
  held back for the runaway retry and everything longer streams from its first chunk, because holding clauses of up
  to nine words delayed their first audio by 0.3 to 0.6 s; 0 restores the earlier rule.
- The Call translation window shows whether the outgoing translation is still being sent, so the user knows when it
  is safe to stop or speak again.
- `call_harness` options for re-translation, the hold limit and the jitter buffer, a per-unit output file, and report
  columns for held versus streamed clauses.
- End-to-end tests of the draft worker and of re-translation.

#### Changed
- The default jitter buffer (`pipeline.jitter_buffer_ms`) is lowered from 150 to 100 ms, since streamed clauses spent
  90 to 150 ms in the pre-roll and no extra gaps were measured at 100 ms.
- The units of one long sentence are aged from the end of the sentence, so the backlog and barge-in rules no longer
  drop early parts of a sentence that is still being spoken.
- The recognition stage is split into `early` and `clips` modules (no behaviour change).
- Documents that are not program documentation (trial checklists, measurement records, the manual driver test plan,
  the stale dependency baseline, the early UI concept and landing pages, a statistics script) were moved out of the
  repository; the native and driver READMEs keep only the build and packaging reference.

#### Fixed
- The length clock restarts when a round's text waits as a carry; before, the length rule fired on every frame,
  finalised and reset the recogniser each time and counted each new filler reading as a dropped unit.
- A rewritten partial is cut at the committed boundary found by content rather than by byte offset, so the cut can no
  longer fall inside a word.
- A Chinese re-translation candidate is not cut inside a word: the cut moves back while shorter candidates translate
  the same, and an unaligned remainder never starts inside a word.
- A stray one-word English fragment at the end of a segment is dropped instead of being spoken.
- The enumeration comma (、) is no longer treated as a clause boundary, so lists are not cut into fragments.

### 2026-11-21

#### Added
- The Call translation window (from the menu-bar menu and the orb's menu) runs a translated call: state, Start and
  Stop, Pause and Resume, the languages, issues with their actions and live captions of both directions; the Debug
  window keeps the raw diagnostics.
- The window was then rebuilt as a conversation: a status chip, one scrolling timeline of bubbles (the translation
  first, the original under it, live partials, the clause being played highlighted, skipped clauses struck), follow
  and jump-to-latest, copy and clear, a remembered text size, empty states, a footer with levels and delay, and a
  details disclosure; Space pauses and resumes, and Translate is on by default.
- The window also shows the translation engine, the graceful stop, the timing of the spoken notice and audible gaps.
- System translation over Apple's on-device Translation framework, through a Swift bridge with a C ABI, one session
  per direction, reporting honestly that it takes no context or instructions.
- `call.translation_engine` (`system` or `llm`), with system translation the default on macOS; configuration schema
  version 4 moves existing settings to it and keeps their language-model entries. The engine can be chosen, with its
  status and actions, in the settings and in the call window, and is reported by preflight and `--check-call`.
- The application manages the local `llama-server`: it starts, health-checks, restarts and stops only its own process,
  adopts a compatible server it did not start (and never stops that one), and stops its own after an idle time
  (`llm_server.idle_stop_s`, default 60 s).
- Language rules: speech already in the listener's language is passed through untranslated
  (`pipeline.skip_source_in_target`), and a translation in the wrong language is asked for once more before anything
  of it is spoken (`pipeline.retry_wrong_language`); every assessment flag is counted and shown.
- One log line per finished utterance and a call summary at stop (figures only, no spoken text); the synthesis engine
  and the voice log when they finish loading.
- A spoken notice to the other party, in their language, that the call is machine-translated with a synthetic voice
  (`call.disclosure`, off by default); it plays as the user's first speech begins, so the first translation does not
  wait for the whole notice.
- Graceful stop: the first Stop lets committed speech be translated and played to the end, up to `call.stop_grace_ms`
  (default 6 s), because Stop used to cut the last sentence.
- Catch-up with queued translated speech: silence at clause ends is trimmed and, once a queue builds, finished clauses
  play up to 1.25x faster with pitch kept (an in-repository WSOLA time scaler), without dropping anything
  (`pipeline.catch_up` and related keys).
- Engine-aware early commit (`pipeline.early_commit_without_context`, default `punctuation`): an engine without
  context gets whole clauses, because mid-clause fragments came back as odd words.
- Self-echo guard: the outgoing direction drops its own recent translation when the microphone hears it again.
- Filler-only units ("嗯", "Mm.", "Uh", punctuation only) are no longer translated or spoken (`pipeline.drop_filler`,
  default on), and starts of speech below `pipeline.min_speech_dbfs` (default -75) are not decoded, because the
  recogniser writes "嗯" for non-speech and such units were being spoken; both are counted and traced.
- "End assistant and start": ends an assistant session that holds the microphone and starts the call with the same
  options once the microphone is free.
- Privacy guard: call captions stay in memory and tests fail if they gain a route to storage; the transcript setting
  says it covers assistant conversations only.
- A dialogue port for a streaming conversational backend, and an OpenAI-compatible dialogue adapter that shares the
  translator's HTTP and SSE client.
- A dialogue role in the provider registry (`providers.active.dialogue`), with the local `llama-server` as default and
  an entry for the future OpenPRX agent that reports "not available yet".
- The assistant shows a visible Starting state and ignores clicks while it loads, writes one log line per turn and a
  session summary (figures only), and keeps its engines loaded for a minute after a session (`assistant.keep_warm_s`)
  so a second session starts without loading.
- `scripts/dev/assistant-harness.sh`: the assistant's loop on recorded speech with the real providers and a latency
  report, without a microphone or speakers.
- `call_harness` can run inputs without speech, profile the VAD and measure early-commit units and final
  disagreements.
- A layout guard test that fails on overlapping or escaping widgets.

#### Changed
- The voice assistant runs on the call translation's pieces: the microphone through Silero VAD and Qwen3-ASR
  (streaming partials in the transcript, the filler filter), the dialogue port with the local `llama-server` and
  Gemma 4 E4B (thinking off, a per-session history bounded by turns and tokens, a short system prompt for a spoken
  assistant), the clause splitter and Qwen3-TTS in a preset voice or the user's own, played through the existing output
  with the echo gate. It is a small explicit state machine (idle, listening, thinking, speaking); speaking over the
  reply or a click cuts it, and a second click ends the session.
- Every problem of the assistant (a model not installed, a runtime not built in, a server that does not answer, a
  missing voice, a refused microphone) is a plain-language issue under the orb with a button to the settings page that
  fixes it, in English and Simplified Chinese; nothing falls back to a stand-in.
- Configuration schema version 3 adds `[assistant]` (language, voice, preset, end-of-turn silence, history bounds)
  and moves `network.language` and `audio.end_silence_ms` into it; `[network]`, `audio.merge_window_ms`,
  `audio.min_speech_ms` and selections of the retired models are dropped on the next save.
- Single-pass streaming recognition (`pipeline.recognition_mode = "streaming"`, Qwen3-ASR 0.6B) is the default; the
  larger refine model is loaded only in two-pass mode, saving memory and time at a measured accuracy cost. Schema 4
  moves a saved `two_pass`, the old default, to streaming once.
- Early commit works together with the refine pass, keeps a date and its year together and cuts Chinese more safely.
- The translation prompt sends the clause inside `<source>` tags with worked examples and asks once more when a reply
  opens like an answer or a refusal, so questions and requests are translated rather than answered.
- The time scaler searches coarse to fine, an eighth of the work, so a whole clause never stalls the playback pump.
- A fully synthesised clause is never dropped as stale, and a streaming clause whose producer is well ahead may be
  sped up.
- The speaker-output issue says that a loudspeaker in the room is heard and translated again.

#### Fixed
- Text with nothing to say aloud (punctuation only) is never sent to the synthesiser.
- A short clause that runs away is held until it ends inside its generation cap and retried once, so a runaway is not
  played; the cap scales with the text, and a clause cut by the cap twice gets a last attempt at twice the cap, so slow
  speech is accepted. (Which clauses are held was narrowed on 2026-11-22.)
- A filler in the middle of a segment joins the next unit instead of being dropped.
- Short or mixed-language utterances are translated, and the original voice is never left switched on after the
  speech.
- An empty refine result keeps the streaming text and no longer counts towards the `RefineFailing` degradation.
- Only audible gaps of the translated speech are counted, and the pre-roll adapts to how fast synthesis delivers,
  because the old underrun count summed counters the listener never hears.
- The process tap is no longer rebuilt for silence once signal was proven, because a call app's output is silent
  while the other side is quiet and the rebuild interrupted capture.
- The time scaler never cuts past the end of its buffer when a stream ends, which panicked the pump thread; every
  thread panic is now logged.
- The virtual microphone is never left as the default input outside call mode.
- A schema migration is written once with the original file kept, and later loads are silent.
- A stopped call leaves no caption live.
- The call window lays out as one vertical flow with a compact header and an options menu.
- The warm-up's synthesis requests are counted apart from the session's.
- The call coordinator's log line names the real previous state of a transition.
- A second click on the orb while the assistant listens ends the session.

#### Removed
- Whisper completely: `whisper-cpp-plus`, the Whisper recogniser, the Whisper VAD, the `whisper` feature of
  `prxvc-audio`, the catalogue entries `whisper-small` and `silero-vad-5.1.2`, the incremental VAD window, the legacy
  audio loop, endpointing and turn policy, and the examples that used them.
- The built-in demo backend: the application never builds `MockGateway` (it is a test double behind the feature
  `test-double`), `network.use_mock` is gone, and so are the prx-voice server transports (`prx_voice_transport`,
  `ws_transport`) with their tests and dependencies. The desktop no longer depends on the engine of `prxvc-core`,
  which only the mobile skeleton (`prxvc-ffi`) and the test kit still use.

### 2026-11-20

#### Added
- Live sessions with the prx-voice server (since removed, see 2026-11-21), assistant audio playback with a shared
  output core and streaming resampler, and a half-duplex echo gate (`audio.echo_control`); settings, history and
  debug open in their own movable windows.
- RF design-standard theme (`prxvc-desktop-ui::theme`): colour, type, radius and spacing tokens for light and dark,
  applied to egui globally; it is the default theme and follows the system light/dark preference, and the previous
  colour themes stay selectable.
- Bundled UI fonts: Hanken Grotesk (400, 500, 600), JetBrains Mono (400, 600) and a GB2312 subset of Noto Sans SC
  (400, 600), wired into the theme's text styles with the system CJK font as fallback; licences in
  `THIRD_PARTY_NOTICES.md`, and `scripts/build-fonts.sh` reproduces the font files.
- A versioned configuration schema with ordered migrations, atomic writes, the app identifier
  `org.openprx.prx-voice-client` and a one-time move from the legacy directories; schema version 2 adds the call
  translation keys.
- macOS application bundle `PRX Voice.app` (`scripts/package-macos.sh`, `scripts/run-app.sh`, `packaging/macos/`):
  microphone and audio-capture usage strings in English and Simplified Chinese, a placeholder icon, ad-hoc or
  Developer ID signing, optional notarization, packaged by the release workflow; `prxvc-desktop --version` and
  `--check`.
- File logging in the application log directory when not attached to a terminal (size-capped, rotated); the app
  warns at start when it runs outside a bundle.
- The `prxvc-models` crate: model catalogue, model store and a resumable, verified download engine with a bounded
  event subscription.
- `NetworkPolicy::check` for the offline switch.
- Microphone choice by explicit device, with a policy that avoids virtual devices.
- `prxvc-media` (shared pipeline value types, now also the clause splitter) and `prxvc-ports` (stage ports with
  scriptable fakes).
- Pure call, direction and segment state machines with commit and interrupt policies, each covered by cross-product
  tests.
- `qwentts.cpp` and `transcribe.cpp` vendored at pinned commits as symbol-isolated native runtimes, so both ggml
  copies coexist in one process; both are bundled and checked.
- The PRX Voice virtual microphone driver and its tools in the repository; the USB variant is built and carried in
  the bundle, and the app installs, updates and uninstalls it.
- `virtual_mic::VirtualMicSink` writes into the driver's hidden sink opened by UID (`audio.virtual_mic_device`, empty
  means our driver; any other output device can be used by entering its UID), and `DriverStatus` reports whether the
  driver is installed, its variant and version, whether the audio service loaded it and whether the version matches.
- Process tap digital-zero watchdog, consent request path and call-activity detection.
- A mixer with modes, crossfades and a jitter buffer.
- Call-mode routing controller with a journal, crash recovery and change listeners, run at start.
- Call path pump, supervised microphone source and microphone consent; an audio-only call coordinator composed with
  the real endpoints and routing, and a live loopback test through the virtual microphone.
- Provider registry for the call roles with typed problems and shared engines; the user's voice is prepared through
  it, and recognition and VAD adapters are built through it.
- `QwenTts` in `prxvc-adapters`: in-process Qwen3-TTS synthesis behind the synthesis port, with a worker pool serving
  two requests at once, per-request bounded chunk queues and cancellation, a generation cap derived from the text that
  aborts a runaway as `Runaway`, a warm-up at load (retried if it runs away), and prepared voices cached on disk per
  talker model; the raw FFI wrapper is behind the cargo feature `qwentts`.
- The TTS catalogue entry is pinned to the files `qwentts.cpp` was measured with, and the 0.6B model is added as the
  speed fallback.
- Silero VAD and a Qwen3-ASR recogniser that produces partials by re-decoding, over `transcribe.cpp`, with catalogue
  entries; a recogniser may end a segment itself at its maximum length.
- The Gemma 4 E4B translation catalogue entry with a `llama-server` template, and a streaming OpenAI-compatible
  translation client with prompts and output checks; a looping translation is aborted early and flagged as suspect.
- The direction pipeline: recognise, refine, translate and synthesise threads around a control thread, with bounded
  queues and a cancellation tree, tested end to end on fakes.
- Both call directions are translated: the outgoing one into the virtual microphone and the incoming one to the local
  output, with captions, counters, latency figures and an opt-in trace file.
- Early commit of translatable units from the recogniser's stable prefix while the speaker talks.
- A settings window with section navigation, stored through atomic updates, with render, interaction, string and
  mapping tests.
- `call_harness`, development helpers and `--check-call` for setting up and checking a first translated call.

#### Changed
- egui and eframe upgraded to 0.37, and repaints follow state instead of running unconditionally.
- `prxvc-audio` capture and playback no longer lock or allocate on the device callbacks: the microphone writes into a
  preallocated lock-free ring and the output drains a lock-free playback ring, behind the `AudioSource` and
  `AudioSink` ports; the old `AudioInput`, `AudioOutput`, `AppAudioSource` and `VirtualMicSink` traits are gone, and a
  test fails when real-time code (`*_rt.rs`) uses a lock, an allocation, logging or a channel send.
- The process tap is hardened: mute mode parameter, private tap, lock-free ring and a stall watchdog.
- Recognised text is no longer logged at `info` (character counts only; the text is logged at `debug`), and the
  default log filter is `info` when `RUST_LOG` is unset.
- The metrics registry is an owned instance, so tests no longer share state.
- `main.rs` and the desktop UI and session `lib.rs` files are split by responsibility, and wrong-direction and unused
  dependencies are removed from audio, desktop UI and storage (no behaviour change).
- `prxvc-audio` builds without default features.
- Committed text is kept when a final recognition result disagrees with it.
- The backlog rule measures how far behind the listener is and never drops the next clause silently.

#### Fixed
- The global hotkey triggers on key press only.
- The resampler no longer starts streams about 8 ms early.

#### Removed
- The sink that located the BlackHole virtual device by name; the virtual microphone endpoint replaces it.
- The Hy-MT2 translation catalogue entry.

### 2026-06-24

#### Added
- Bilingual consecutive-interpreting state machine and turn driver (`prxvc-session`).
- macOS per-application audio capture via Core Audio process taps, with 16 kHz mono resampling (`prxvc-audio`).
- `prxvc-adapters` crate: machine-translation and speech-synthesis adapters, including an OpenAI-compatible
  translator and a development tone synthesizer.

#### Changed
- Repository URLs point at the openprx organization, and a security contact is set.

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
