#!/usr/bin/env bash
# Print, without changing anything, whether everything for a first translated call is in place:
# the bundle and its signature, the native runtimes, the virtual microphone driver, the
# consents (as the bundle sees them), the default input and output (and whether the output is the
# built-in speaker), the models, the owner's voice, the preset voice, the selected translation
# engine (system translation: its language packs; language model: the local server, which the
# application starts by itself) and the state of the local model server.
#
# Usage: scripts/dev/preflight.sh [--app "PATH TO PRX Voice.app"]
#   default app: dist/debug/PRX Voice.app (built by scripts/run-app.sh --native --no-open)
# Exit status: 0 when nothing is missing.
set -uo pipefail
. "$(dirname "$0")/_common.sh"

app="$root/dist/debug/PRX Voice.app"
if [ "${1:-}" = "--app" ]; then app="${2:?--app needs a path}"; fi
missing=0
say() { printf '%s\n' "$*"; }
miss() { say "[MISSING] $*"; missing=$((missing + 1)); }

say "== bundle =="
if [ ! -d "$app" ]; then
  miss "bundle: $app does not exist (scripts/run-app.sh --native --no-open builds it)"
else
  say "[ok     ] bundle: $app"
  if codesign --verify --deep --strict "$app" 2>/dev/null; then
    authority="$(codesign -dvv "$app" 2>&1 | grep -m1 '^Authority=' || true)"
    say "[ok     ] signature: valid (${authority:-ad-hoc})"
  else
    miss "signature: invalid or missing (scripts/package-macos.sh signs with PRXVC_CODESIGN_IDENTITY=\"PRX Voice Development\")"
  fi
  id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist" 2>/dev/null || true)"
  [ "$id" = "$app_id" ] && say "[ok     ] identifier: $id" || miss "identifier: '$id', expected $app_id"
  for lib in qwentts transcribe apple_translation; do
    if ls "$app/Contents/Frameworks/"*"$lib"* >/dev/null 2>&1; then
      say "[ok     ] native library: $lib in Contents/Frameworks"
    else
      miss "native library: $lib not in the bundle (build it with --native)"
    fi
  done
  say ""
  say "== the bundle's own report (--check-call) =="
  bin="$app/Contents/MacOS/prxvc-desktop"
  # A bundle built before the flag existed would treat it as "start the window": never run it then.
  if ! grep -q -a -e '--check-call' "$bin"; then
    miss "the bundle is older than the source and does not know --check-call: rebuild it (scripts/run-app.sh --native --no-open)"
  else
    out="$("$bin" --check-call 2>&1)"; status=$?
    say "$out" | grep -v -E '^\[(GGML|Qwen)' || true
    [ "$status" -eq 0 ] || miss "the bundle's --check-call exited with $status"
    missing=$((missing + $(printf '%s\n' "$out" | grep -c '^\[MISSING\]' || true)))
  fi
fi

say ""
say "== helper report (native runtimes loadable, models, voices, translation engine) =="
out="$(run_dev preflight 2>&1)"; status=$?
say "$out"
missing=$((missing + $(printf '%s\n' "$out" | grep -c '^\[MISSING\]' || true)))

say ""
say "== local model server (only needed when the language model is the engine; the app starts it itself) =="
"$(dirname "$0")/llama-server.sh" status || true

say ""
if [ "$missing" -eq 0 ]; then say "preflight: everything is in place"; else say "preflight: $missing item(s) missing"; fi
[ "$missing" -eq 0 ]
