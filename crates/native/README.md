# Native runtimes

This tree holds the two native runtimes of the call-translation pipeline, vendored and built as
**one dynamic library per runtime, each with its own ggml linked statically inside and only its own C API
exported** (pipeline-design PD-2.7.2):

| Crate | Runtime | Library | Exports | Cargo feature |
|---|---|---|---|---|
| `qwentts-sys` (`prxvc-qwentts-sys`) | `qwentts.cpp` text to speech, commit `2edb35f87b709a070b2d5379f77e5c52a1daa585`, ggml fork `ef4555b` (version 1.0.0) | `libprxvc_qwentts.dylib` | 18 `_qt_*` symbols | `qwentts` |
| `transcribe-sys` (`prxvc-transcribe-sys`) | `transcribe.cpp` ASR and VAD, commit `275cd50d2c4104c204009a197ac091b9687668e5`, ggml `d7cb574` (version 0.26.0, four downstream patches) | `libprxvc_transcribe.dylib` | 147 `_transcribe_*` symbols | `transcribe` |
| `link` (`prxvc-native-link`) | the spike itself: thin owning wrappers and the tests below | none | none | `qwentts`, `transcribe` |

## How to run

```text
# Default workspace builds never touch C++ (the features are off):
cargo check --workspace --all-targets
cargo test --workspace

# Build both runtimes and run the checks that need no model and no GPU
# (exported symbols, dependencies, struct layouts, versions):
cargo test -p prxvc-native-link --features qwentts,transcribe --test native

# Live checks (models used in place from bench/, GPU, Metal), one at a time:
cargo test -p prxvc-native-link --features qwentts,transcribe --release --test live \
    -- --ignored --nocapture --test-threads=1
# PRXVC_BENCH_ROOT overrides /Users/nn/worker/voice/bench. Nothing is played or recorded.
```

Needs `cmake` and `clang` on `PATH` (Command Line Tools plus the Homebrew cmake already on the machine; nothing
was installed). No new crate dependency: the build scripts call `cmake` and `clang++` directly instead of using
the `cmake` crate, and the spike crate depends on `std` only.

## Vendoring decision

Followed the design: **vendored source trees in the repository at the pinned commits, not fetched at build time.**

| Tree | Files | Bytes | Left out |
|---|---|---|---|
| `qwentts-sys/vendor/qwentts.cpp` (`src/`, `ggml/`, `LICENSE`) | 233 | 8.0 MB | `tools/`, `tests/`, `vendor/` (httplib, yyjson), docs; ggml examples, tests, docs, every backend but CPU, Metal, BLAS |
| `transcribe-sys/vendor/transcribe.cpp` (`include/`, `src/`, `cmake/`, `ggml/`, `CMakeLists.txt`, licence files) | 463 | 12.4 MB | `bindings/`, `examples/`, `tests/`, `tools/`, `docs/`, `samples/`, `reports/`, models, catalogue; the same ggml trimming |

About 20 MB and 696 files added to the repository (the full upstream checkouts are 124 MB and 677 MB with
history and build output). Why vendoring:

- Reproducible offline builds, which the design asks for; no network and no checksum scheme at build time.
- The vendored tree is exactly what was measured. A fetch script would add a second thing that can fail or drift.
- Licence notices travel with the code (`LICENSE`, `ggml/LICENSE`, miniz and Silero notices are inside the trees;
  `THIRD_PARTY_NOTICES.md` at the repository root now lists them).
- The cost is size, and the two ggml copies are about 5 MB each. A single shared ggml is not possible anyway:
  the two ggml trees differ in 99 files and `ggml.h` and `ggml-backend.h` differ in their API (446 diff lines).

`VENDOR.toml` in each crate records the upstream URL, the pinned commit and what was left out.
`scripts/vendor.sh <qwentts|transcribe> <checkout>` refreshes a tree from a local checkout at the pinned
commit (it refuses any other commit) and edits nothing. The design names `scripts/vendor-qwentts.sh`; that
path can be a one-line wrapper around this script. Nothing under `vendor/` is edited; two consequences:
`qt_version()` reports `2edb35f (2026-11-17)` through a build definition (the trimmed tree has no `.git`), and
the ggml build prints the repository commit as its own `ggml commit` (cosmetic).

For CS-3.1: 35 vendored C, C++ and header files exceed 3,000 lines (for example `ggml-cpu/ops.cpp`, 12,453). The conformance
script must exclude `**/vendor/**`. Files written for this package are all under 800 lines.

## Build

`build.rs` of each `-sys` crate (shared logic in `build_common.rs`, pulled in with `include!`), only when its
feature is on:

1. `cmake -S ... -B $OUT_DIR/cmake-build` Release, position independent code, `-DGGML_METAL=ON`,
   `-DGGML_METAL_EMBED_LIBRARY=ON` (shaders embedded in the library, no `.metallib` sidecar), the target
   architecture and `MACOSX_DEPLOYMENT_TARGET` (14.4 when unset, `package-macos.sh` exports 14.4).
   `GGML_NATIVE=OFF`, `GGML_OPENMP=OFF` so the shipped binary does not depend on the build host's CPU.
   - `transcribe.cpp`: upstream's own `CMakeLists.txt` unmodified (`TRANSCRIBE_BUILD_SHARED=OFF`, tests and
     examples off), target `transcribe`.
   - `qwentts.cpp`: our `qwentts-sys/native/CMakeLists.txt` instead of upstream's, because upstream's also builds
     the CLI tools, the HTTP server and the tests whose sources are not vendored. It builds the `qwen-core`
     target with the definitions upstream uses (`GGML_MAX_NAME=128`, `_FORTIFY_SOURCE=2`, C++17) and the ggml
     options of the measured build (Metal, Accelerate BLAS).
2. All sources are compiled with hidden visibility into static archives (`libqwen-core.a` or `libtranscribe.a`
   plus `libggml*.a`).
3. `clang++ -dynamiclib` links the archives (`-force_load` on the runtime archive) with
   `-exported_symbols_list native/exports.txt` (`_qt_*`, `_transcribe_*`) and `-dead_strip`, install name
   `@rpath/lib<name>.dylib`, system frameworks Accelerate, Foundation, Metal, MetalKit and `libc++`.
4. The library is copied to `target/<profile>/native-libs/` (where the bundle script takes it from) and the
   crate emits `rustc-link-search`, `rustc-link-lib=dylib` and `DEP_PRXVC_*_LIB_DIR` (through `links =`).
   `link/build.rs` turns the latter into an absolute run-time `-rpath` for test binaries.

Build time of the native part on this machine (18 cores, clean, one profile): `qwentts` 10.0 s wall (31 s CPU),
`transcribe` 13.9 s wall (87 s CPU), about 24 s wall in total; the two libraries are 7.0 MB and 8.6 MB.
A clean `cargo check -p prxvc-native-link -p prxvc-qwentts-sys -p prxvc-transcribe-sys` without the features takes
0.8 s; the rest of `cargo check --workspace --all-targets` is unchanged (50 s clean, no warnings, 0 warnings in a
repeat run).

## Bindings

Hand-written, for exactly the surface in use, each declared item documented; the ownership and threading rules
are in the crate-level docs (CS-2.10). `qwentts-sys`: `qt_init` / `qt_free`, `qt_extract_voice_ref` /
`qt_voice_ref_free`, `qt_synthesize` with `on_chunk` and `cancel`, batching through `qt_init_params::max_batch`,
`qt_audio_free`, logging, version. `transcribe-sys`: model load, session init, one-shot run with text and
segments, streaming begin / feed / finalize / text, abort callback, log callback, ABI self-check.
Layout checks: `transcribe_abi_struct_size` / `_align` against `size_of` / `align_of` for seven structs;
`qt_*_default_params` values against the header (`native` test target).

`link/src/tts.rs` and `link/src/asr.rs` are the minimum wrappers the tests need: owning types with `Drop`
(`qt_free` is mandatory, otherwise the Metal backend aborts at exit), an `Arc` from every session to its model so
"the model outlives its sessions" holds by construction, `catch_unwind` in every callback trampoline, the
chunk callback documented as running on the library's compute worker thread. The adapters of later packages
replace them.

## Packaging

- **Where.** `PRX Voice.app/Contents/Frameworks/libprxvc_qwentts.dylib` and `libprxvc_transcribe.dylib`.
  The bundle script already signs everything in `Contents/Frameworks` first (`*.dylib` loop) and then the bundle.
- **How they are found.** Install names are `@rpath/lib<name>.dylib`. The executable needs the run-time search path
  `@executable_path/../Frameworks`. Development builds get an absolute rpath into
  `target/<profile>/build/prxvc-*-sys-*/out` from the build script of the crate that links the `-sys` crate
  (`link/build.rs` is the pattern: `cargo:rustc-link-arg=-Wl,-rpath,$DEP_PRXVC_QWENTTS_LIB_DIR`).
- **Implemented (2026-11-20).** `apps/desktop` has the feature `qwentts` (forwarding to `prxvc-adapters/qwentts`; the
  adapters now also have the feature `transcribe` for the second library, `libprxvc_transcribe.dylib`, which the
  desktop app has to forward and give the same development rpath and bundle step as the first, see "Second library" below) and a `build.rs` that emits the development rpath
  (`target/<profile>/native-libs`, derived from `OUT_DIR`, because `DEP_*` variables reach direct dependents only)
  only with the feature on. `scripts/package-macos.sh --native` (or `PRXVC_NATIVE=1`; `scripts/run-app.sh --native`)
  copies the libraries the executable links through `@rpath`, rewrites the rpath, signs them before the bundle and
  checks rpath, signatures, dependencies and `--check` (which prints a `native:` line per loadable library).
- **Signing.** (A self-signed identity has no Team ID and needs `disable-library-validation`; the script adds it for that case only, see `packaging/macos/README.md`.) Each library must be signed with the same identity as the executable (Developer ID with
  `--timestamp --options runtime`, as the script does for nested code). Under the hardened runtime, library
  validation rejects a library with a different Team ID: measured here, an executable signed
  `--options runtime` ad hoc next to ad-hoc libraries fails at launch with `not valid for use in process: mapping
  process and mapped file (non-platform) have different Team IDs`. So the ad-hoc path of the script must keep
  skipping the hardened runtime (it does), and a Developer ID build needs libraries signed in the same run. No
  extra entitlement should be needed (no JIT; Metal shaders are compiled from the embedded source with the
  public API), but this could not be checked here: the machine has no signing identity. Check it on the first
  Developer ID build with the notarisation run.
- **Architectures and deployment target.** `build.rs` passes the target architecture (`arm64` or `x86_64`)
  and `MACOSX_DEPLOYMENT_TARGET` (14.4 from the script) to CMake and the link. Only arm64 was built and run here;
  an `x86_64` build (and a universal one, which would need `lipo`) is untested.
- **Size.** 7.0 MB and 8.6 MB added to the bundle.

## Second library in the adapters (CT-11)

`prxvc-adapters` has the cargo feature `transcribe = ["dep:prxvc-transcribe-sys", "prxvc-transcribe-sys/transcribe"]`
(additive, off by default). With it on, `vad::SileroVad` and `asr::QwenAsr` exist, `build.rs` of the adapters crate adds the
development rpath of `libprxvc_transcribe.dylib` next to the one of `libprxvc_qwentts.dylib`, and the only `unsafe` for this
runtime is in `crates/adapters/src/transcribe/`. For the bundle: an executable that links `prxvc-adapters` with `transcribe`
needs the same two things the TTS library needed, in `apps/desktop`: a feature forwarding to `prxvc-adapters/transcribe` and a
`build.rs` rpath line for `target/<profile>/native-libs`; `scripts/package-macos.sh --native` already copies every library the
executable links through `@rpath`, so `libprxvc_transcribe.dylib` (8.6 MB) is signed and copied with the first one. Loading both
models at start: the Qwen3-ASR load takes about 0.15 s once the Metal pipelines are cached and the Silero load (CPU) a few ms;
the first launch after a rebuild compiles the embedded shaders (about 2.7 s). Two direction pipelines share one `QwenAsr` (one
model, one worker thread) and open one VAD session each on one Silero model.

## Keeping ordinary builds fast

The `-sys` crates and `link` are workspace members so the default `cargo check` and `cargo test` cover their
Rust (the unit tests of the WAV reader, resampler and word error rate run in the default test run: +11 tests),
but the native build happens only with the crate features `qwentts` and `transcribe` (cost without them: 0.8 s on a
clean check). Later packages wire the design's feature names through the adapters crate:
`qwentts = ["dep:prxvc-qwentts-sys", "prxvc-qwentts-sys/qwentts"]`, `transcribe = ["dep:prxvc-transcribe-sys",
"prxvc-transcribe-sys/transcribe"]`; the features are additive, as CS-7.5 requires. `cargo check --all-features`
builds both runtimes (24 s clean).

## Apple translation bridge (`apple-translation-sys`)

A third library, different in kind: not a vendored C++ runtime but a small first-party Swift library
(`native/*.swift`, about 300 lines) with a C ABI (`native/prxvc_apple_translation.h`) over Apple's
on-device Translation framework. Status: system translation, the default translation engine on macOS.

| Crate | Library | Exports | Cargo feature |
|---|---|---|---|
| `apple-translation-sys` (`prxvc-apple-translation-sys`) | `libprxvc_apple_translation.dylib` | 11 `_prxat_*` symbols | `apple-translation` |

Build: `build.rs` runs `swiftc` from the Command Line Tools (no Xcode, no cmake) with
`-target <arch>-apple-macos<MACOSX_DEPLOYMENT_TARGET>` (14.4 unless set), `-swift-version 5`, `-O`,
install name `@rpath/libprxvc_apple_translation.dylib`, `-exported_symbols_list native/exports.txt` and
`-dead_strip`. The Translation framework is linked weakly (`-weak_framework`) and every use sits behind
`#available(macOS 26.0)` (the strategy parameter behind `macOS 26.4`), so the app launches on macOS
14.4 and the bridge answers `PRXAT_OS_TOO_OLD`, which the adapter turns into the typed problem
"needs macOS 26". The library depends only on system libraries (`/usr/lib/swift`, Foundation,
Translation), so the bundle check's dependency rule holds unchanged. The library is copied to
`target/<profile>/native-libs/` like the others; `scripts/package-macos.sh --native` bundles, signs and
checks it (`PRXVC_NATIVE_FEATURES="qwentts transcribe apple-translation"`), and `--check` prints
`native: apple-translation bridge abi 1, ...`.

Threading contract (also in the header and in the `-sys` crate docs):

- Every function is thread-safe. `prxat_prepare`, `prxat_translate`, `prxat_availability` and
  `prxat_create` block the calling thread until the answer, the timeout or `prxat_cancel`; never call them
  from a device callback. Calls on one session are serialised; `prxat_cancel` may come from any thread.
- The framework delivers its work through the **main dispatch queue**. The process's main thread must run
  its run loop: the application does (winit/AppKit); a command-line tool or test binary calls
  `prxat_run_main_loop` on its main thread and works on another thread (`crates/adapters/tests/apple_translation_live.rs`,
  `call_harness`, `prxvc-dev`, the bundle's `--check-call`: all through `prxvc_adapters::mt::apple::ffi::run_with_main_loop`). Without it the framework never answers; the bridge detects this in about one second and
  returns `PRXAT_NO_MAIN_LOOP`.
- After a timeout the bridge replaces its internal framework session, so the next call starts clean.
- Errors are codes plus a message; strings returned by the library are freed with `prxat_free`.
