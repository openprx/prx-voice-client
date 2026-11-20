#!/usr/bin/env bash
# Install the owner's voice profile and a development preset voice, set them in the configuration
# when none is set, and prepare the owner's voice for the synthesis model.
#
#   owner-zh, owner-en   the owner's two confirmed reference recordings with their texts
#                        (bench/tts/refs/zh_owner.*, en_owner.*), embedding-only mode
#   dev-standin          a STAND-IN preset voice for what the owner hears (Chinese), made with
#                        the system synthesizer (bench/tts/refs/zh_ref.wav); dev-standin-en for
#                        English. Local test material: not a product voice, never shipped.
#
# The configuration is backed up (config.toml.bak-<time>) before it is changed, and only
# voice.self_profile_id and voice.incoming_preset are set, and only when empty. The file is
# written back by the app's own writer, which also adds every default key it did not hold.
#
# Usage: scripts/dev/install-voices.sh [--refs DIR] [--no-prepare]
set -euo pipefail
. "$(dirname "$0")/_common.sh"
run_dev install-voices "$@"
