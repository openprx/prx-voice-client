#!/usr/bin/env bash
# Build the PRX Voice virtual microphone driver bundle(s): compile, assemble, sign, structural checks.
# No admin rights needed; nothing is installed.
#
# Usage: driver/macos/build.sh [--transport virtual|usb|builtin|other|all] [--out DIR]
#   --transport V  transport type the driver reports to applications (default: usb, the variant to ship)
#                    virtual  kAudioDeviceTransportTypeVirtual  'virt'
#                    usb      kAudioDeviceTransportTypeUSB      'usb '
#                    builtin  kAudioDeviceTransportTypeBuiltIn  'bltn'
#                    other    kAudioDeviceTransportTypePCI      'pci '  (non-zero, in no skip list)
#   --out DIR      output root (default: driver/macos/build); bundles land in DIR/<variant>/
#
# Version: the file driver/macos/VERSION (X.Y.Z). CFBundleShortVersionString is "<version>-<variant>",
# CFBundleVersion is X*10000 + Y*100 + Z, the same rule as the app bundle.
#
# Signing: PRXVC_CODESIGN_IDENTITY selects the identity (as in scripts/package-macos.sh); unset means
# ad-hoc. The host-side load test is run by scripts/check-driver.sh.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT="$HERE/build"
ALL_VARIANTS="virtual usb builtin other"
VARIANT_ARG="usb"
NAME="PrxVoiceMicrophone.driver"

die() { echo "error: $*" >&2; exit 1; }
usage() { sed -n '2,17p' "${BASH_SOURCE[0]}"; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --transport) VARIANT_ARG="${2:-}"; shift 2 ;;
    --transport=*) VARIANT_ARG="${1#--transport=}"; shift ;;
    --out) OUT="${2:?--out needs a directory}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 1 ;;
  esac
done
if [[ "$VARIANT_ARG" == "all" ]]; then VARIANTS="$ALL_VARIANTS"; else VARIANTS="$VARIANT_ARG"; fi

variant_const() {
  case "$1" in
    virtual) echo kAudioDeviceTransportTypeVirtual ;;
    usb) echo kAudioDeviceTransportTypeUSB ;;
    builtin) echo kAudioDeviceTransportTypeBuiltIn ;;
    other) echo kAudioDeviceTransportTypePCI ;;
    *) return 1 ;;
  esac
}
variant_fourcc() {
  case "$1" in virtual) echo "virt" ;; usb) echo "usb " ;; builtin) echo "bltn" ;; other) echo "pci " ;; esac
}
for v in $VARIANTS; do
  variant_const "$v" >/dev/null || die "unknown transport variant '$v' (virtual|usb|builtin|other|all)"
done

VERSION="$(tr -d '[:space:]' < "$HERE/VERSION")"
case "$VERSION" in [0-9]*.[0-9]*.[0-9]*) ;; *) die "cannot read X.Y.Z from $HERE/VERSION" ;; esac
IFS=. read -r v_major v_minor v_patch <<<"$VERSION"
BUILD_NUMBER=$(( v_major * 10000 + v_minor * 100 + v_patch ))

IDENTITY="${PRXVC_CODESIGN_IDENTITY:--}"
sign() {
  if [[ "$IDENTITY" == "-" ]]; then
    codesign --force --sign - "$@"
  else
    codesign --force --timestamp --options runtime --sign "$IDENTITY" "$@"
  fi
}

PB=/usr/libexec/PlistBuddy
mkdir -p "$OUT"
for v in $VARIANTS; do
  CONST="$(variant_const "$v")"
  FOURCC="$(variant_fourcc "$v")"
  VDIR="$OUT/$v"
  BUNDLE="$VDIR/$NAME"
  BIN="$BUNDLE/Contents/MacOS/PrxVoiceMicrophoneDriver"
  echo "=== variant: $v ($CONST '$FOURCC'), version $VERSION-$v"

  rm -rf "$VDIR"
  mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
  sed -e "s|@VERSION@|$VERSION-$v|g" -e "s|@BUILD_NUMBER@|$BUILD_NUMBER|g" \
    "$HERE/Info.plist.in" > "$BUNDLE/Contents/Info.plist"
  # NOTICE cannot sit directly in Contents/ (codesign reports an unsealed subcomponent).
  cp "$HERE/NOTICE" "$BUNDLE/Contents/Resources/NOTICE"
  $PB -c "Add :PrxTransportVariant string $v" "$BUNDLE/Contents/Info.plist"
  $PB -c "Add :PrxTransportType string $FOURCC" "$BUNDLE/Contents/Info.plist"

  clang -std=c11 -O2 -Wall -Wextra -Werror -bundle \
    -arch arm64 -mmacosx-version-min=13.0 \
    -DPRX_TRANSPORT_TYPE="$CONST" \
    -framework CoreAudio -framework CoreFoundation \
    "$HERE/PrxVoiceMicrophoneDriver.c" -o "$BIN"

  sign "$BUNDLE"
  echo "signed with: $([[ "$IDENTITY" == "-" ]] && echo ad-hoc || echo "$IDENTITY")"

  echo "--- verify ($v)"
  plutil -lint "$BUNDLE/Contents/Info.plist"
  codesign --verify --strict --verbose=2 "$BUNDLE"
  file "$BIN"
  lipo -archs "$BIN" | grep -qx arm64 || die "$BIN is not arm64"
  echo "arch: arm64"
  nm -gU "$BIN" | grep -q ' _PrxVoiceMicrophoneDriverCreate$' || die "factory symbol missing in $BIN"
  echo "factory symbol: _PrxVoiceMicrophoneDriverCreate"
  [[ "$($PB -c 'Print :PrxTransportVariant' "$BUNDLE/Contents/Info.plist")" == "$v" ]] || die "variant key mismatch"
  echo "version: $($PB -c 'Print :CFBundleShortVersionString' "$BUNDLE/Contents/Info.plist")," \
       "variant: $($PB -c 'Print :PrxTransportVariant' "$BUNDLE/Contents/Info.plist")"
done
echo "built variants: $VARIANTS in $OUT"
