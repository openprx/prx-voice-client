# Development helpers for this machine

These scripts set up and check what a first translated call needs. They are **development helpers
for this machine, not part of the product**: they know where the models and recordings of this
workspace live, install a stand-in voice and start the translation server by hand (decision D-P6:
the owner or a script starts it for now). Nothing here is bundled or shipped.

Everything is written inside the application's own data directory
(`~/Library/Application Support/org.openprx.prx-voice-client/`, or `$PRXVC_DATA_DIR`), except the
build output under `target/`.

| Script | What it does |
|---|---|
| `adopt-models.sh` | Installs the models of the default call plan (Silero VAD, Qwen3-ASR 0.6B and 1.7B, Qwen3-TTS 1.7B with its codec, Gemma 4 E4B) into the model store through the store's own adopt path: APFS clones of the existing files are checked against the catalogue's size and SHA-256 pins and moved into the store with a receipt. The source files stay where they are. Safe to run again. Needs the app closed. |
| `install-voices.sh` | Installs the owner's two confirmed recordings as the profiles `owner-zh` and `owner-en` (embedding-only), a stand-in preset `dev-standin` (Chinese; `dev-standin-en` for English) made from the system-synthesizer recordings, sets `voice.self_profile_id = owner` and `voice.incoming_preset = dev-standin` when they are empty (after backing up `config.toml`), and prepares the owner's voice for the synthesis model. |
| `llama-server.sh start\|stop\|status` | Starts the translation server as the default provider expects it (Gemma 4 E4B as `gemma-4-e4b-it-qat-q4-0` on `http://127.0.0.1:8080/v1`, two slots, 8192 context, all layers on the GPU), with a PID file, a health wait and a `stop` that only stops what the script started. |
| `assistant-harness.sh` | Runs the voice assistant's loop on recorded speech without a microphone or speakers: the real providers and the local dialogue server, the reply's audio written to a file, and a latency table (end of speech to final text, first reply token, first reply audio). |
| `preflight.sh` | Prints, changing nothing, whether everything is in place: bundle and signature, native libraries, driver, consents (as the bundle sees them), default input and output and whether the output is the built-in speaker, models, owner's voice, preset voice, translation server. |

All of them build `prxvc-dev` (release, features `qwentts transcribe`) on first use; set
`PRXVC_DEV_BIN` to use another build. The native runtimes print their own log lines; the scripts
filter those out.

## Where the files come from

`adopt-models.sh` looks, after any `--source DIR` and `$PRXVC_DEV_SOURCES` (colon separated), in
`bench/asr/models/hf`, `bench/tts/models` and `code/prx-voice/models/llm` of this workspace, and
matches files by catalogue name and exact size. `install-voices.sh` reads
`bench/tts/refs/{zh_owner,en_owner,zh_ref,en_ref}.{wav,txt}` (`--refs DIR` changes the folder). The
stand-in recordings are local test material made with the system synthesizer: never commit, bundle
or ship them, and never use them as the owner's voice.

## Order for a first test

1. `scripts/dev/adopt-models.sh`, then `scripts/dev/install-voices.sh`.
2. `scripts/dev/llama-server.sh start`.
3. `scripts/run-app.sh --native --no-open`, then `scripts/dev/preflight.sh`.
4. Follow `docs/call-translation-test-checklist.md`.
5. `scripts/dev/llama-server.sh stop` when done.
