#!/usr/bin/env bash
# Build the macOS test tooling: the prx-vmic feeder/recorder/self-test tool and the host-side
# driver load test. Needs the Command Line Tools only; no admin rights, nothing is installed.
#
# Usage: scripts/build-macos-tools.sh [--out DIR]
#   --out DIR   output root (default: tools/macos/build)
# Output: DIR/bin/prx-vmic and DIR/driver_loadtest. The build directory is git-ignored.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/tools/macos/build"
while [ $# -gt 0 ]; do
  case "$1" in
    --out) out="$(mkdir -p "${2:?--out needs a directory}" && cd "$2" && pwd)"; shift ;;
    -h|--help) sed -n '2,8p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done
[ "$(uname -s)" = Darwin ] || { echo "error: the macOS tools build on macOS only" >&2; exit 1; }

mkdir -p "$out/bin"
echo "building driver_loadtest"
clang -std=c11 -Wall -Wextra -Werror -Wno-unused-parameter "$root/tools/macos/driver_loadtest.c" \
  -o "$out/driver_loadtest" -framework CoreAudio -framework CoreFoundation
echo "building prx-vmic"
make -s -C "$root/tools/macos/prx-vmic" OUT="$out/bin/prx-vmic"
echo "built: $out/bin/prx-vmic, $out/driver_loadtest"
