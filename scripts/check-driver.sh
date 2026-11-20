#!/usr/bin/env bash
# Repository check for the virtual microphone driver: builds every transport variant and runs the
# host-side load test on each (the transport type, the device UIDs, the format, the ring loopback and
# the latency cap). No audio daemon, no installation, no admin rights. Skipped on non-macOS hosts.
#
# Usage: scripts/check-driver.sh
# Output goes to a temporary directory that is removed on exit.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
if [ "$(uname -s)" != Darwin ]; then
  echo "check-driver: skipped (macOS only)"
  exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail() { echo "check-driver: FAIL: $*" >&2; exit 1; }
PB=/usr/libexec/PlistBuddy

{ "$root/scripts/build-macos-tools.sh" --out "$work/tools" && \
  "$root/driver/macos/build.sh" --transport all --out "$work/driver"; } >"$work/build.log" 2>&1 \
  || { cat "$work/build.log" >&2; fail "build failed"; }

version="$(tr -d '[:space:]' < "$root/driver/macos/VERSION")"
for v in virtual usb builtin other; do
  case "$v" in virtual) cc="virt" ;; usb) cc="usb " ;; builtin) cc="bltn" ;; other) cc="pci " ;; esac
  bundle="$work/driver/$v/PrxVoiceMicrophone.driver"
  bin="$bundle/Contents/MacOS/PrxVoiceMicrophoneDriver"
  plist="$bundle/Contents/Info.plist"
  [ "$($PB -c 'Print :PrxTransportVariant' "$plist")" = "$v" ] || fail "$v: variant key"
  [ "$($PB -c 'Print :PrxTransportType' "$plist")" = "$cc" ] || fail "$v: transport key"
  [ "$($PB -c 'Print :CFBundleShortVersionString' "$plist")" = "$version-$v" ] || fail "$v: version string"
  [ -f "$bundle/Contents/Resources/NOTICE" ] || fail "$v: NOTICE missing in the bundle"
  echo "=== load test: $v (expects transport '$cc')"
  "$work/tools/driver_loadtest" "$bin" "$cc" | tee "$work/$v.log" | grep -E '^(FAIL|PASSED|FAILED)' || true
  grep -q '^PASSED (0 failures)' "$work/$v.log" || fail "$v: host-side load test did not pass"
done

# Negative control: the load test must reject a binary whose transport differs from the expectation.
if "$work/tools/driver_loadtest" "$work/driver/usb/PrxVoiceMicrophone.driver/Contents/MacOS/PrxVoiceMicrophoneDriver" virt >/dev/null 2>&1; then
  fail "the load test accepted a usb binary as virtual"
fi
echo "negative control: a usb binary tested against 'virt' fails, as it should"
echo "check-driver: OK (4 variants, version $version)"
