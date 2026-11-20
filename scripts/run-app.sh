#!/usr/bin/env bash
# Build a debug bundle (same layout as the release one, ad-hoc signed) and
# open it, so that development happens inside a bundle: macOS then attributes
# microphone and call-audio permission to "PRX Voice" and not to the terminal.
#
# Usage: scripts/run-app.sh [--no-open] [--native] [-- app arguments]
#   --no-open   build and verify the bundle, do not launch it
#   --native    build with the native runtime features and bundle their libraries
#               (same as PRXVC_NATIVE=1; see scripts/package-macos.sh)
#
# The log goes to a file (the app is not attached to a terminal):
#   ~/Library/Application Support/org.openprx.prx-voice-client/logs/prx-voice.log
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
open_app=1
package_args=(--debug)
while [ "${1:-}" = "--no-open" ] || [ "${1:-}" = "--native" ]; do
  case "$1" in
    --no-open) open_app=0 ;;
    --native) package_args+=(--native) ;;
  esac
  shift
done
[ "${1:-}" = "--" ] && shift

"$root/scripts/package-macos.sh" "${package_args[@]}"

app="$root/dist/debug/PRX Voice.app"
[ "$open_app" = 1 ] || { echo "not opening (--no-open)"; exit 0; }

if pgrep -f "$app/Contents/MacOS/prxvc-desktop" >/dev/null; then
  echo "PRX Voice (debug bundle) is already running: quit it from the tray menu, then run this again." >&2
  exit 1
fi
open -n "$app" --args "$@"
echo "opened $app"
echo "log: $HOME/Library/Application Support/org.openprx.prx-voice-client/logs/prx-voice.log"
