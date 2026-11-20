#!/usr/bin/env bash
# Report whether the PRX Voice Microphone driver is installed and loaded. No admin rights needed.
set -uo pipefail

DEST="/Library/Audio/Plug-Ins/HAL/PrxVoiceMicrophone.driver"
echo "== input devices (system_profiler SPAudioDataType)"
profile="$(system_profiler SPAudioDataType 2>/dev/null)"
# Print each device that has input channels.
echo "$profile" | awk '/^        [^ ].*:$/ {name=$0} /Input Channels:/ {print name; print $0}'

echo "== PRX Voice Microphone"
if [[ -d "$DEST" ]]; then
  ver="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$DEST/Contents/Info.plist" 2>/dev/null)"
  build="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$DEST/Contents/Info.plist" 2>/dev/null)"
  var="$(/usr/libexec/PlistBuddy -c 'Print :PrxTransportVariant' "$DEST/Contents/Info.plist" 2>/dev/null)"
  echo "bundle   : installed, version $ver (build $build) at $DEST"
  echo "variant  : ${var:-virtual (older bundle without a variant key)} (from the installed Info.plist)"
  codesign -dv "$DEST" 2>&1 | grep -E "Signature|Authority|TeamIdentifier" | sed 's/^/signature : /'
else
  echo "bundle   : not installed"
fi
if echo "$profile" | grep -q "PRX Voice Microphone:"; then
  echo "device   : PRESENT (loaded by the audio daemon)"
  TOOL="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/tools/macos/build/bin/prx-vmic"
  if [[ -x "$TOOL" ]]; then
    echo "== live properties (prx-vmic props; this is what applications see)"
    "$TOOL" props | sed 's/^/live     : /'
  else
    echo "live     : tools/macos/build/bin/prx-vmic missing; run scripts/build-macos-tools.sh to get the live transport query"
  fi
  exit 0
fi
echo "device   : not present"
exit 1
