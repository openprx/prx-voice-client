# Native runtimes (work package CT-9, the native link spike)

Status: spike result, 2026-11-20. Machine: Apple M5 Max, macOS 26.6, Metal.

This tree holds the two native runtimes of the call-translation pipeline, vendored and built as
**one dynamic library per runtime, each with its own ggml linked statically inside and only its own C API
exported** (pipeline-design PD-2.7.2):

| Crate | Runtime | Library | Exports | Cargo feature |
|---|---|---|---|---|
| `qwentts-sys` (`prxvc-qwentts-sys`) | `qwentts.cpp` text to speech, commit `2edb35f87b709a070b2d5379f77e5c52a1daa585`, ggml fork `ef4555b` (version 1.0.0) | `libprxvc_qwentts.dylib` | 18 `_qt_*` symbols | `qwentts` |
| `transcribe-sys` (`prxvc-transcribe-sys`) | `transcribe.cpp` ASR and VAD, commit `275cd50d2c4104c204009a197ac091b9687668e5`, ggml `d7cb574` (version 0.26.0, four downstream patches) | `libprxvc_transcribe.dylib` | 147 `_transcribe_*` symbols | `transcribe` |
| `link` (`prxvc-native-link`) | the spike itself: thin owning wrappers and the tests below | none | none | `qwentts`, `transcribe` |

Result in one sentence: the two runtimes load into one process, run concurrently on Metal, give correct
output and exit cleanly, with no duplicate-class warning and no collision, so the fallbacks of PD-2.7.2
(a shared ggml, a sidecar, `sherpa-onnx`) are not needed for linking reasons.

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

## Symbol isolation result

`nm -gU` on the built libraries (release build):

```text
libprxvc_qwentts.dylib:    18 exported symbols, 0 matching ggml|gguf (case-insensitive)
  _qt_audio_free _qt_duration_sec_to_tokens _qt_extract_voice_ref _qt_free _qt_init
  _qt_init_default_params _qt_language_name _qt_last_error _qt_log_set _qt_model_type
  _qt_n_languages _qt_n_speakers _qt_num_codebooks _qt_speaker_name _qt_synthesize
  _qt_tts_default_params _qt_version _qt_voice_ref_free
libprxvc_transcribe.dylib: 147 exported symbols, all `_transcribe_*`, 0 matching ggml|gguf
```

The non-ignored tests `qwentts_library_exports_only_its_c_api`, `transcribe_library_exports_only_its_c_api`
(every exported symbol starts with the runtime's prefix, none contains `ggml` or `gguf`) and
`libraries_depend_only_on_system_libraries_and_themselves` (`otool -L`: only `@rpath/<self>`, `/System/Library/`,
`/usr/lib/`) enforce this. Each library still holds about 900 private `ggml_` / `gguf_` symbols
(`nm` without `-g`): two complete ggml copies live in the process.

- **ggml versions differ and it is harmless.** `qwentts.cpp` pins a fork, `ServeurpersoCom/ggml` at `ef4555b`
  (version 1.0.0); `transcribe.cpp` pins upstream `ggml-org/ggml` at `d7cb574` (0.26.0) with four patches.
  Their public headers differ, so one shared ggml would need a port; isolation makes the difference irrelevant
  because neither copy is visible outside its library.
- **Objective-C class clash: not present.** Both ggml-metal copies define `GGMLMetalClass`, but only when
  `GGML_METAL_EMBED_LIBRARY` is off (it is a handle to find the `.metallib` bundle). With the library embedded
  the class is not compiled, so there is nothing for the runtime to complain about. In every process run (more than a dozen) the
  output contains no `objc` or "implemented in both" line. Keep `GGML_METAL_EMBED_LIBRARY=ON` (set by `build.rs`).
- **Metal.** Each ggml has its own device object, command queue, residency set and embedded shader library; both
  report `MTL0 (Apple M5 Max)`. The GPU serialises the work; see the timings below for the cost.
- **Negative control** (not shipped, scratch only): relinking both libraries without the exported-symbols list
  (72 and 70 `ggml_` symbols then exported) and running the same concurrent test with `DYLD_LIBRARY_PATH`
  also works. macOS two-level namespaces already keep same-named symbols of different dylibs apart. The export
  list is therefore defence in depth against weak-symbol coalescing and future flat-namespace consumers, and it
  is what the design's acceptance check measures; the part that matters for safety is that the ggml copies are
  inside the dylibs and not in the executable (the `whisper-cpp-plus` ggml, linked into the executable, is the one
  to remove in CT-20).

## Concurrency and exit result

`tests/live.rs::tts_and_asr_run_concurrently_and_round_trip` (ignored). One process: `qwentts.cpp` 1.7B Q8_0
(`max_batch = 2`, owner's English reference, speaker-embedding mode, seed fixed, `max_new_tokens` from the
text length: 0.28 s per word times 3 in 80 ms frames, floor 40) and `transcribe.cpp` with Qwen3-ASR 0.6B Q8_0, both
on Metal. Steps: load both, warm-up synthesis and decode, one solo synthesis (baseline in the same process),
then the synthesis of `The meeting has been moved to Thursday afternoon, so please update your calendar before
the call.` while a second thread decodes the owner's reference WAV in a loop (at least two decodes, until the
synthesis ends), then the synthesized audio (24 kHz resampled to 16 kHz) through the ASR, then everything freed
in dependency order. Asserts: audio between 0.5x and 2x the expected duration, finite and not silent; every
concurrent ASR result within 0.15 word error rate of the known transcript; round trip within 0.25; return
code 0. TTFA is the time from the `qt_synthesize` call to the first chunk callback.

Ten runs in a row (separate processes, release build). Batch A ran with nothing on the GPU but an idle
`llama-server`; batch B ran while the e2e prototype (`bench/e2e`, a TTS worker and an LLM server) and a
benchmark were active (their load is not quantified).

| Run | TTFA solo (ms) | TTFA concurrent (ms) | RTF solo | RTF concurrent | ASR decodes during synthesis | ASR decode median (ms) | Round-trip WER |
|---|---|---|---|---|---|---|---|
| A1 | 37.3 | 42.5 | 0.196 | 0.232 | 10 | 140 | 0.000 |
| A2 | 36.8 | 41.7 | 0.193 | 0.256 | 9 | 150 | 0.000 |
| A3 | 37.1 | 45.4 | 0.200 | 0.321 | 9 | 200 | 0.000 |
| A4 | 36.8 | 47.7 | 0.195 | 0.291 | 10 | 173 | 0.000 |
| A5 | 36.8 | 46.8 | 0.196 | 0.294 | 10 | 182 | 0.000 |
| B1 | 88.9 | 43.7 | 0.215 | 0.306 | 10 | 195 | 0.000 |
| B2 | 36.5 | 54.2 | 0.216 | 0.404 | 9 | 246 | 0.000 |
| B3 | 83.9 | 70.6 | 0.420 | 0.348 | 9 | 191 | 0.000 |
| B4 | 37.4 | 46.8 | 0.194 | 0.292 | 10 | 172 | 0.000 |
| B5 | 37.6 | 46.2 | 0.228 | 0.367 | 9 | 224 | 0.000 |

All ten exit with status 0, no Metal assertion, no warning. Solo, in the Rust process: TTFA 36.5 to 37.6 ms
(the harness measured about 40 ms) and RTF 0.19 to 0.20 on a quiet GPU (the harness: about 0.2): the FFI path adds
nothing measurable. With the recognizer decoding at the same time TTFA rises by 5 to 10 ms and RTF to 0.23 to
0.32 (0.35 to 0.40 in the busier batch); the recognizer's decode of 7 s of audio takes 110 to 160 ms alone and
140 to 250 ms during synthesis. Both stay far inside real time. The ASR text equals the transcript exactly
(WER 0.000 in every concurrent decode and in all round trips; the round trip returns the sentence with a changed
punctuation only).

Other ignored tests, all passing in one process (`--test-threads=1`, which creates and frees five engines
and models in sequence): TTS alone (TTFA 35.6 to 52.8 ms, RTF 0.19 to 0.25), cancel through the `cancel` callback
(status `-5` 336 ms after a 300 ms request, engine usable afterwards), Qwen3-ASR alone (WER 0.000, 104 to 128 ms for 6.96 s),
and the streaming API with the Nemotron streaming model (first text after chunk 6 of 44 of 160 ms,
final WER 0.000, 494 ms for the whole 6.96 s).

Cold start: the first launch after a rebuild compiles the embedded Metal shaders of each ggml copy, about 2.7 s for each
(`ggml_metal_library_compile_all: loaded 36 libraries from embedded data in 2.755 sec`): TTS load 3.6 s and
ASR load 2.5 s in the first process, against 0.6 to 0.9 s and about 0.1 s afterwards (the system caches the
compiled pipelines). The app should load the engines in parallel with startup work and show a "preparing" state.

## ASR runtime recommendation, linking point of view

**`transcribe.cpp` can be the ASR runtime and the VAD runtime** (`TranscribeVad`, PD-2.5.1) in the same
process as `qwentts.cpp`. Nothing in linking forces the `sherpa-onnx` fallback and no sidecar process is
needed for isolation. Conditions: keep the two-dylib shape with static ggml and the export lists, keep the Metal
library embedded, load both libraries from `Contents/Frameworks`, and remove `whisper-cpp-plus` (a third ggml,
linked into the executable) from the call build as planned (CT-20) so that no ggml sits in the executable.
For completeness: the prebuilt `sherpa-onnx` C API and `libonnxruntime` dylibs of the benchmark venv
(read-only, not used or modified) load into the same process as the two libraries here (a `dlopen` check only;
`libsherpa-onnx-c-api` exports 189 symbols and no ggml, `libonnxruntime` exports 3), so `sherpa-onnx` stays a
valid per-model alternative for models `transcribe.cpp` does not run well; that decision is a latency and
accuracy one, not a linking one.

## Packaging

- **Where.** `PRX Voice.app/Contents/Frameworks/libprxvc_qwentts.dylib` and `libprxvc_transcribe.dylib`.
  The bundle script already signs everything in `Contents/Frameworks` first (`*.dylib` loop) and then the bundle.
- **How they are found.** Install names are `@rpath/lib<name>.dylib`. The executable needs the run-time search path
  `@executable_path/../Frameworks`. Development builds get an absolute rpath into
  `target/<profile>/build/prxvc-*-sys-*/out` from the build script of the crate that links the `-sys` crate
  (`link/build.rs` is the pattern: `cargo:rustc-link-arg=-Wl,-rpath,$DEP_PRXVC_QWENTTS_LIB_DIR`).
- **Change to `scripts/package-macos.sh` (not made here).** After the `cargo build` step: take the libraries
  from `$bin_dir/$profile/native-libs/` (`bin_dir` already covers `--target`), copy them into
  `$contents/Frameworks/`, then on the copied executable run `install_name_tool -delete_rpath` for every
  `LC_RPATH` that is not `@`-relative and `install_name_tool -add_rpath @executable_path/../Frameworks`
  (the application crate may instead emit that rpath itself with `cargo:rustc-link-arg-bins` and skip the
  development one when `PRXVC_BUNDLE=1`). Both must happen before the existing signing section, because
  `install_name_tool` invalidates a signature. Add `codesign --verify` of each library and an
  `otool -L` check that no path points outside the bundle or the system. The notices step copies the updated
  `THIRD_PARTY_NOTICES.md` (already does).
  Checked here on a scratch bundle layout (executable in `Contents/MacOS`, libraries in `Contents/Frameworks`,
  rpath rewritten as above, ad-hoc signed): the concurrent test ran from inside it with identical output.
- **Signing.** Each library must be signed with the same identity as the executable (Developer ID with
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

## Keeping ordinary builds fast

The `-sys` crates and `link` are workspace members so the default `cargo check` and `cargo test` cover their
Rust (the unit tests of the WAV reader, resampler and word error rate run in the default test run: +11 tests),
but the native build happens only with the crate features `qwentts` and `transcribe` (cost without them: 0.8 s on a
clean check). Later packages wire the design's feature names through the adapters crate:
`qwentts = ["dep:prxvc-qwentts-sys", "prxvc-qwentts-sys/qwentts"]`, `transcribe = ["dep:prxvc-transcribe-sys",
"prxvc-transcribe-sys/transcribe"]`; the features are additive, as CS-7.5 requires. `cargo check --all-features`
builds both runtimes (24 s clean).

## Open issues

- Hardened-runtime launch with a Developer ID identity and notarisation of the two libraries: not testable here.
- `x86_64` and universal builds: code path written, not run.
- The two GPU users contend: with the recognizer running at the same time the synthesizer's RTF rose from 0.20 to
  0.23 to 0.32 on a quiet GPU. Fine for one direction pair, but the two-direction pipeline plus the MT model
  needs the C1b measurement of the voice-cloning plan, not this spike.
- Cold start of the embedded shaders (about 2.7 s per ggml copy on the first launch after a rebuild; whether it
  recurs after a macOS update or on another Mac was not measured).
- `qt_version()` and the ggml "commit" strings are set through build definitions because the vendored trees have no
  `.git`; refresh `QWEN_VERSION_STRING` in `qwentts-sys/build.rs` together with `VENDOR.toml`.
- Not measured: memory of the two engines together, long-running stability (hours), and the `sherpa-onnx` path
  beyond the load check.
- Pre-existing and outside this package: `cargo clippy --workspace --all-targets -- -D warnings` fails in
  `prxvc-models` and `cargo fmt --check` reports differences in other crates (both on the base commit too).
