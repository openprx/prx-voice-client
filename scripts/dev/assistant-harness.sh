#!/usr/bin/env bash
# Run the voice assistant's loop on recorded speech, silently: a WAV file is the microphone, the real
# providers (Silero VAD, Qwen3-ASR, the dialogue server, Qwen3-TTS) run, and the reply's audio is
# written to a file. No microphone is opened and nothing is played. Prints, per utterance and as the
# median, the time from the end of the speech to the final text, the first reply token and the first
# reply audio.
#
# Usage: scripts/dev/assistant-harness.sh [assistant_harness arguments]
#   e.g. scripts/dev/assistant-harness.sh --set en --subset en_read --count 4 --lang en --preset dev-standin-en
#        scripts/dev/assistant-harness.sh --set zh --subset short --count 5
# See the header of apps/desktop/src/bin/assistant_harness/main.rs for every option.
# Needs the models (adopt-models.sh), a preset voice (install-voices.sh) and the dialogue server
# (llama-server.sh start). Builds the program (release, native features) on first use; set
# PRXVC_ASSISTANT_HARNESS to use another build.
set -euo pipefail
. "$(dirname "$0")/_common.sh"

curl -sf --max-time 2 "http://127.0.0.1:${LLAMA_PORT:-8080}/health" >/dev/null 2>&1 \
  || echo "note: the dialogue server does not answer; the run will report it (scripts/dev/llama-server.sh start)" >&2

bin="${PRXVC_ASSISTANT_HARNESS:-}"
if [ -z "$bin" ]; then
  (cd "$root" && cargo build --release -q -p prxvc-desktop --bin assistant_harness \
    --features "qwentts transcribe") >&2
  bin="$root/target/release/assistant_harness"
fi
set +e
"$bin" "$@" 2>&1 | grep -v -E '^\[|^(qwen3_asr|silero_vad|ggml|qwentts)[^ ]*:|^ggml_metal'
status=${PIPESTATUS[0]}
exit "$status"
