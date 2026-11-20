#!/usr/bin/env bash
# Install the models of the default call plan into the app's model store from the files already
# on this machine, through the store's own adopt path (sizes and SHA-256 are checked against the
# catalogue pins, the store layout is the one the app expects). The source files are cloned
# (APFS, no extra disk) and never moved. Safe to run again: installed models are left alone.
#
# Usage: scripts/dev/adopt-models.sh [--dry-run] [--source DIR]...
#   Sources searched, after any --source: $PRXVC_DEV_SOURCES (colon-separated), then
#   bench/asr/models/hf, bench/tts/models and code/prx-voice/models/llm of this workspace.
#   The app must not be running (it holds the store).
set -euo pipefail
. "$(dirname "$0")/_common.sh"
run_dev adopt-models "$@"
