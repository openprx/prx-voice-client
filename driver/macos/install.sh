#!/usr/bin/env bash
# Install the PRX Voice Microphone driver. Run with admin rights:
#   sudo ./install.sh [--transport virtual|usb|builtin|other] [-y]      (default variant: usb)
# The variant must have been built first (driver/macos/build.sh --transport V or --transport all).
# Only one variant is installed at a time (same bundle id and device UID): installing replaces the current one.
# Safe to rerun: it replaces only our own bundle. It touches no other file under /Library.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NAME="PrxVoiceMicrophone.driver"
BUNDLE_ID="org.openprx.prx-voice-client.microphone-driver"
VARIANT="usb"
ASSUME_YES=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --transport) VARIANT="${2:-}"; shift 2 ;;
    --transport=*) VARIANT="${1#--transport=}"; shift ;;
    -y) ASSUME_YES=1; shift ;;
    *) echo "usage: sudo $0 [--transport virtual|usb|builtin|other] [-y]" >&2; exit 1 ;;
  esac
done
case "$VARIANT" in virtual|usb|builtin|other) ;; *) echo "error: unknown transport variant '$VARIANT' (virtual|usb|builtin|other)" >&2; exit 1 ;; esac
SRC="$HERE/build/$VARIANT/$NAME"
HAL="/Library/Audio/Plug-Ins/HAL"
DEST="$HAL/$NAME"
STAGING="$HAL/.$NAME.staging"

[[ "$(id -u)" -eq 0 ]] || { echo "error: needs admin rights. Run: sudo $0" >&2; exit 1; }
[[ -d "$SRC" ]] || { echo "error: the '$VARIANT' variant is not built ($SRC not found). Run: driver/macos/build.sh --transport $VARIANT" >&2; exit 1; }
found_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$SRC/Contents/Info.plist")"
[[ "$found_id" == "$BUNDLE_ID" ]] || { echo "error: refusing to install bundle id '$found_id' (expected $BUNDLE_ID)" >&2; exit 1; }
codesign --verify --strict "$SRC" || { echo "error: bundle signature is invalid; rebuild with driver/macos/build.sh" >&2; exit 1; }

src_variant="$(/usr/libexec/PlistBuddy -c 'Print :PrxTransportVariant' "$SRC/Contents/Info.plist" 2>/dev/null || true)"
[[ "$src_variant" == "$VARIANT" ]] || { echo "error: bundle says variant '${src_variant:-none}', expected '$VARIANT'; rebuild with driver/macos/build.sh --transport $VARIANT" >&2; exit 1; }
if [[ -d "$DEST" ]]; then
  cur="$(/usr/libexec/PlistBuddy -c 'Print :PrxTransportVariant' "$DEST/Contents/Info.plist" 2>/dev/null || true)"
  cur_ver="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$DEST/Contents/Info.plist" 2>/dev/null || true)"
  installed_desc="${cur:-virtual (older bundle without a variant key)} (version $cur_ver)"
else
  installed_desc="none"
fi
src_ver="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$SRC/Contents/Info.plist")"
echo "Variant about to be installed : $VARIANT (version $src_ver)"
echo "Variant currently installed   : $installed_desc"
echo

cat <<PLAN
This will:
  1. copy   $SRC
     to     $STAGING   (staging copy)
  2. set owner root:wheel and mode a+rX on the staging copy
  3. remove $DEST (only if it exists) and move the staging copy there
  4. restart the audio daemon: launchctl kickstart -k system/com.apple.audio.coreaudiod
     (fallback: killall coreaudiod). All audio on this Mac drops out for a few seconds.
  5. list the audio devices to confirm "PRX Voice Microphone" appears
PLAN
if [[ -z "$ASSUME_YES" ]]; then
  read -r -p "Proceed? [y/N] " ans
  [[ "$ans" == "y" || "$ans" == "Y" ]] || { echo "aborted"; exit 1; }
fi

mkdir -p "$HAL"
rm -rf "$STAGING"
ditto "$SRC" "$STAGING"
chown -R root:wheel "$STAGING"
chmod -R a+rX "$STAGING"
rm -rf "$DEST"
mv "$STAGING" "$DEST"
echo "installed: $DEST (variant: $VARIANT)"

launchctl kickstart -k system/com.apple.audio.coreaudiod || killall coreaudiod
echo "audio daemon restarted; waiting for devices..."
for _ in 1 2 3 4 5 6 7 8 9 10; do
  sleep 1
  if system_profiler SPAudioDataType 2>/dev/null | grep -q "PRX Voice Microphone"; then
    echo "OK: PRX Voice Microphone is present:"
    system_profiler SPAudioDataType | grep -A9 "PRX Voice Microphone:"
    echo "Run driver/macos/status.sh to confirm the live transport type (expected variant: $VARIANT)."
    exit 0
  fi
done
echo "WARNING: PRX Voice Microphone did not appear after 10 s." >&2
echo "The driver was copied but coreaudiod did not load it. Check: log show --last 2m --predicate 'process == \"coreaudiod\"' | grep -i -E 'prx|plug|sign'" >&2
exit 2
