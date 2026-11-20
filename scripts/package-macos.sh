#!/usr/bin/env bash
# Build "PRX Voice.app" from prxvc-desktop. Plain shell plus the tools of the
# Command Line Tools (cargo, plutil, codesign, iconutil, sips, ditto); no Xcode.
#
# Usage: scripts/package-macos.sh [--debug] [--target TRIPLE] [--skip-build] [--zip]
#
#   --debug        debug build, bundle in dist/debug/ (default: release, in dist/)
#   --target T     cargo target triple (default: the host architecture)
#   --skip-build   reuse the binary already built for this profile and target
#   --zip          also write dist/PRX-Voice-<version>-<arch>.zip (ditto, keeps signature)
#
# Environment:
#   PRXVC_CODESIGN_IDENTITY   signing identity (e.g. "Developer ID Application: Name (TEAMID)").
#                             Unset: ad-hoc signature (-), no hardened runtime.
#   PRXVC_NOTARY_PROFILE      notarytool keychain profile; with a Developer ID identity the
#                             bundle is notarized and stapled. Alternatively set
#                             PRXVC_NOTARY_KEY (path to the .p8), PRXVC_NOTARY_KEY_ID and
#                             PRXVC_NOTARY_ISSUER. Never run unless set.
#
# See packaging/macos/README.md.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

APP_NAME="PRX Voice"
EXECUTABLE="prxvc-desktop"
PACKAGE="prxvc-desktop"
# Process taps need 14.2; the tap shim (crates/audio/native/prxvc_tap.m) is
# written against 14.4 (@available(macOS 14.4, *)).
MIN_OS="14.4"
export MACOSX_DEPLOYMENT_TARGET="$MIN_OS"

profile=release
target=""
skip_build=0
make_zip=0
while [ $# -gt 0 ]; do
  case "$1" in
    --debug) profile=debug ;;
    --target) target="${2:?--target needs a triple}"; shift ;;
    --skip-build) skip_build=1 ;;
    --zip) make_zip=1 ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

die() { echo "error: $*" >&2; exit 1; }

# Single sources of truth: the version from Cargo, the identifier from
# prxvc_config::paths::APP_ID (the same constant names the data directory).
pkgid="$(cargo pkgid -p "$PACKAGE")"
version_full="${pkgid##*#}"; version_full="${version_full##*@}"
version="${version_full%%[-+]*}"
case "$version" in [0-9]*.[0-9]*.[0-9]*) ;; *) die "cannot read the version from '$pkgid'" ;; esac
IFS=. read -r v_major v_minor v_patch <<<"$version"
build_number=$(( v_major * 10000 + v_minor * 100 + v_patch ))
bundle_id="$(sed -n 's/^pub const APP_ID: .*= ("\([^"]*\)", "\([^"]*\)", "\([^"]*\)");.*/\1.\2.\3/p' \
  crates/config/src/paths.rs)"
[ -n "$bundle_id" ] || die "APP_ID not found in crates/config/src/paths.rs"

# Build for the requested target; the binary lives in a different directory
# when --target is given.
cargo_args=(build --locked -p "$PACKAGE")
[ "$profile" = release ] && cargo_args+=(--release)
bin_dir="target"
if [ -n "$target" ]; then
  cargo_args+=(--target "$target")
  bin_dir="target/$target"
fi
binary="$bin_dir/$profile/$EXECUTABLE"
if [ "$skip_build" = 0 ]; then
  echo "building $PACKAGE ($profile${target:+, $target}, macOS $MIN_OS)"
  cargo "${cargo_args[@]}"
fi
[ -x "$binary" ] || die "$binary not found (build first, or drop --skip-build)"

out="dist"
[ "$profile" = debug ] && out="dist/debug"
bundle="$root/$out/$APP_NAME.app"
contents="$bundle/Contents"
rm -rf "$bundle"
mkdir -p "$contents/MacOS" "$contents/Resources"

cp "$binary" "$contents/MacOS/$EXECUTABLE"

# Info.plist from the template.
sed -e "s|@EXECUTABLE@|$EXECUTABLE|g" \
    -e "s|@BUNDLE_ID@|$bundle_id|g" \
    -e "s|@APP_NAME@|$APP_NAME|g" \
    -e "s|@VERSION@|$version|g" \
    -e "s|@BUILD_NUMBER@|$build_number|g" \
    -e "s|@MIN_OS@|$MIN_OS|g" \
    packaging/macos/Info.plist.in > "$contents/Info.plist"
plutil -lint "$contents/Info.plist" >/dev/null
printf 'APPL????' > "$contents/PkgInfo"

# Localized Info.plist strings.
cp -R packaging/macos/resources/. "$contents/Resources/"

# Icon: 1024 px source -> iconset -> icns.
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
iconset="$work/AppIcon.iconset"
mkdir "$iconset"
src="packaging/macos/icon/AppIcon-1024.png"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$src" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  sips -z "$((size * 2))" "$((size * 2))" "$src" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$contents/Resources/AppIcon.icns"

# Licences and third-party notices.
cp LICENSE LICENSE-MIT LICENSE-APACHE THIRD_PARTY_NOTICES.md "$contents/Resources/"

# Virtual microphone driver (usb variant, the one to ship): built with the same signing identity and
# carried as payload that the app installs into /Library/Audio/Plug-Ins/HAL on request. Its notice
# travels with it (inside the driver bundle and next to the licences).
driver_dir="$contents/Resources/Drivers"
case "${target%%-*}" in
  ""|aarch64|arm64)
    echo "building the virtual microphone driver (usb variant)"
    "$root/driver/macos/build.sh" --transport usb --out "$work/driver" >"$work/driver-build.log" 2>&1 \
      || { cat "$work/driver-build.log" >&2; die "the driver build failed"; }
    mkdir -p "$driver_dir"
    ditto "$work/driver/usb/PrxVoiceMicrophone.driver" "$driver_dir/PrxVoiceMicrophone.driver"
    cp driver/macos/NOTICE "$contents/Resources/NOTICE-PrxVoiceMicrophone-driver"
    ;;
  *) echo "note: the virtual microphone driver is arm64 only; not bundled for target $target" ;;
esac

# Signing. Nested code (the driver payload; native libraries loaded later go in
# Contents/Frameworks) is signed first with the same identity, so library
# validation under the hardened runtime accepts it.
identity="${PRXVC_CODESIGN_IDENTITY:--}"
entitlements="packaging/macos/entitlements.plist"
sign() {
  if [ "$identity" = "-" ]; then
    # Ad-hoc: no hardened runtime. Library validation would reject every
    # library signed differently, and there is no team to compare against.
    codesign --force --sign - "$@"
  else
    codesign --force --timestamp --options runtime --sign "$identity" "$@"
  fi
}
for dir in Frameworks PlugIns Library Resources/Drivers; do
  if [ -d "$contents/$dir" ]; then
    while IFS= read -r nested; do sign "$nested"; done < <(
      find "$contents/$dir" \( -name '*.dylib' -o -name '*.framework' -o -name '*.driver' -o -name '*.bundle' \) -prune)
  fi
done
sign --entitlements "$entitlements" "$bundle"
codesign --verify --deep --strict "$bundle"

# Smoke check from inside the bundle: no window, no audio device.
check="$("$contents/MacOS/$EXECUTABLE" --check)"
expected="$(printf '%s\n' "$check" | sed -n 's/^expected bundle identifier: //p')"
[ "$expected" = "$bundle_id" ] || die "bundle identifier $bundle_id differs from the code's APP_ID ($expected)"
printf '%s\n' "$check" | grep -q "^bundle: .*($bundle_id)" || die "the executable does not see its bundle"

echo "built $bundle (version $version, build $build_number, id $bundle_id, signed: $([ "$identity" = - ] && echo ad-hoc || echo "$identity"))"

notary_args=()
if [ -n "${PRXVC_NOTARY_PROFILE:-}" ]; then
  notary_args=(--keychain-profile "$PRXVC_NOTARY_PROFILE")
elif [ -n "${PRXVC_NOTARY_KEY:-}" ]; then
  notary_args=(--key "$PRXVC_NOTARY_KEY" --key-id "${PRXVC_NOTARY_KEY_ID:?set PRXVC_NOTARY_KEY_ID}" \
    --issuer "${PRXVC_NOTARY_ISSUER:?set PRXVC_NOTARY_ISSUER}")
fi
if [ ${#notary_args[@]} -gt 0 ]; then
  [ "$identity" != "-" ] || die "notarization needs a Developer ID identity (PRXVC_CODESIGN_IDENTITY)"
  echo "notarizing (this waits for Apple)"
  ditto -c -k --keepParent "$bundle" "$work/notarize.zip"
  xcrun notarytool submit "$work/notarize.zip" "${notary_args[@]}" --wait
  xcrun stapler staple "$bundle"
  spctl --assess --type execute --verbose=2 "$bundle"
fi

if [ "$make_zip" = 1 ]; then
  arch="${target%%-*}"; arch="${arch:-$(uname -m)}"
  zip="$root/$out/PRX-Voice-$version-$arch.zip"
  rm -f "$zip"
  ditto -c -k --keepParent "$bundle" "$zip"
  echo "wrote $zip"
fi
