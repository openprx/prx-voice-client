#!/usr/bin/env bash
# Remove the PRX Voice Microphone driver. Run with admin rights:  sudo ./uninstall.sh [-y]
# Safe to rerun: removes only our own bundle and our staging directory.
set -euo pipefail

NAME="PrxVoiceMicrophone.driver"
HAL="/Library/Audio/Plug-Ins/HAL"
DEST="$HAL/$NAME"
STAGING="$HAL/.$NAME.staging"

[[ "$(id -u)" -eq 0 ]] || { echo "error: needs admin rights. Run: sudo $0" >&2; exit 1; }

cat <<PLAN
This will:
  1. remove $DEST (if present)
  2. remove $STAGING (if present)
  3. restart the audio daemon: launchctl kickstart -k system/com.apple.audio.coreaudiod
     (fallback: killall coreaudiod). All audio on this Mac drops out for a few seconds.
No other driver is touched.
PLAN
if [[ "${1:-}" != "-y" ]]; then
  read -r -p "Proceed? [y/N] " ans
  [[ "$ans" == "y" || "$ans" == "Y" ]] || { echo "aborted"; exit 1; }
fi

rm -rf "$DEST" "$STAGING"
launchctl kickstart -k system/com.apple.audio.coreaudiod || killall coreaudiod
sleep 2
if system_profiler SPAudioDataType 2>/dev/null | grep -q "PRX Voice Microphone"; then
  echo "WARNING: PRX Voice Microphone is still listed; give coreaudiod a few more seconds and run status.sh" >&2
  exit 2
fi
echo "OK: removed; PRX Voice Microphone is no longer listed"
