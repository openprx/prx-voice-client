#!/usr/bin/env bash
# Build "PRX Voice.app" from prxvc-desktop. Plain shell plus the tools of the
# Command Line Tools (cargo, plutil, codesign, iconutil, sips, ditto); no Xcode.
#
# Usage: scripts/package-macos.sh [--debug] [--target TRIPLE] [--skip-build] [--zip] [--native]
#
#   --debug        debug build, bundle in dist/debug/ (default: release, in dist/)
#   --target T     cargo target triple (default: the host architecture)
#   --skip-build   reuse the binary already built for this profile and target
#   --zip          also write dist/PRX-Voice-<version>-<arch>.zip (ditto, keeps signature)
#   --native       build with the native runtime features (cargo features in PRXVC_NATIVE_FEATURES,
#                  default "qwentts"), put the dynamic libraries the executable links into
#                  Contents/Frameworks, replace the development rpath by @executable_path/../Frameworks
#                  and sign them with the executable's identity. Default builds are unchanged.
#
# Environment:
#   PRXVC_CODESIGN_IDENTITY   signing identity (e.g. "Developer ID Application: Name (TEAMID)").
#                             Unset: ad-hoc signature (-), no hardened runtime.
#   PRXVC_NOTARY_PROFILE      notarytool keychain profile; with a Developer ID identity the
#                             bundle is notarized and stapled. Alternatively set
#                             PRXVC_NOTARY_KEY (path to the .p8), PRXVC_NOTARY_KEY_ID and
#                             PRXVC_NOTARY_ISSUER. Never run unless set.
#
#   PRXVC_NATIVE              1 is the same as --native.
#   PRXVC_NATIVE_FEATURES     cargo features for --native (default: qwentts).
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
native="${PRXVC_NATIVE:-0}"
native_features="${PRXVC_NATIVE_FEATURES:-qwentts}"
while [ $# -gt 0 ]; do
  case "$1" in
    --debug) profile=debug ;;
    --target) target="${2:?--target needs a triple}"; shift ;;
    --skip-build) skip_build=1 ;;
    --zip) make_zip=1 ;;
    --native) native=1 ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
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
[ "$native" = 1 ] && cargo_args+=(--features "$native_features")
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

# Native runtime libraries (--native): the ones the executable links through @rpath, copied from
# target/<profile>/native-libs into Contents/Frameworks. The development rpath of the build (an
# absolute path into target/) is deleted and replaced by the bundle-relative one. Done before signing:
# install_name_tool invalidates the linker's signature.
exe_in_bundle="$contents/MacOS/$EXECUTABLE"
rpath_libs() { otool -L "$1" | sed -n 's|^[[:space:]]*@rpath/\([^ ]*\.dylib\) .*|\1|p'; }
if [ "$native" = 1 ]; then
  libs="$(rpath_libs "$exe_in_bundle")"
  [ -n "$libs" ] || die "--native: the executable links no @rpath library (features: $native_features)"
  mkdir -p "$contents/Frameworks"
  while IFS= read -r lib; do
    src="$bin_dir/$profile/native-libs/$lib"
    [ -f "$src" ] || die "--native: $src not found (the build did not produce $lib)"
    cp "$src" "$contents/Frameworks/$lib"
  done <<<"$libs"
  while IFS= read -r rp; do
    case "$rp" in /*) install_name_tool -delete_rpath "$rp" "$exe_in_bundle" ;; esac
  done < <(otool -l "$exe_in_bundle" | sed -n 's|^ *path \(.*\) (offset [0-9]*)$|\1|p')
  install_name_tool -add_rpath "@executable_path/../Frameworks" "$exe_in_bundle"
fi

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
# Library validation (hardened runtime) accepts a library only when it has the same Team ID as the
# executable. A self-signed development identity has no Team ID, so nested libraries would be refused at
# load; for that case only, the bundle also carries disable-library-validation. A Developer ID identity
# has a team and keeps the stricter entitlements.
if [ "$identity" != "-" ] && [ -d "$contents/Frameworks" ]; then
  first_lib="$(find "$contents/Frameworks" -name '*.dylib' -prune | sed -n 1p)"
  lib_info="$(codesign -dvv "$first_lib" 2>&1 || true)"
  if [ -n "$first_lib" ] && [[ "$lib_info" == *$'\nTeamIdentifier=not set'* ]]; then
    echo "note: identity has no Team ID; adding disable-library-validation so the bundled libraries load"
    cp "$entitlements" "$work/entitlements.plist"
    /usr/libexec/PlistBuddy -c "Add :com.apple.security.cs.disable-library-validation bool true" "$work/entitlements.plist"
    entitlements="$work/entitlements.plist"
  fi
fi
sign --entitlements "$entitlements" "$bundle"
codesign --verify --deep --strict "$bundle"

# Native libraries: resolved through the bundle-relative rpath only, present, signed like the executable.
if [ "$native" = 1 ]; then
  rpaths="$(otool -l "$exe_in_bundle" | sed -n 's|^ *path \(.*\) (offset [0-9]*)$|\1|p')"
  [ "$rpaths" = "@executable_path/../Frameworks" ] || die "unexpected LC_RPATH entries: $rpaths"
  authority() { codesign -dvv "$1" 2>&1 | sed -n 's/^Authority=//p;s/^Signature=\(adhoc\)$/\1/p' | sed -n 1p; }
  exe_authority="$(authority "$exe_in_bundle")"
  while IFS= read -r lib; do
    [ -f "$contents/Frameworks/$lib" ] || die "$lib is not in Contents/Frameworks"
    codesign --verify --strict "$contents/Frameworks/$lib" || die "$lib is not validly signed"
    [ "$(authority "$contents/Frameworks/$lib")" = "$exe_authority" ] \
      || die "$lib is signed with a different identity than the executable"
    deps="$(otool -L "$contents/Frameworks/$lib" | sed -n '2,$p' | grep -v -e '@rpath/' -e '/System/Library/' -e '/usr/lib/' || true)"
    [ -z "$deps" ] || die "$lib depends on a library outside the system and @rpath: $deps"
  done <<<"$(rpath_libs "$exe_in_bundle")"
  echo "native libraries in Contents/Frameworks (resolved through @rpath):"
  rpath_libs "$exe_in_bundle" | sed 's/^/  /'
  echo "  signature: $exe_authority"
fi

# Smoke check from inside the bundle: no window, no audio device.
check="$("$contents/MacOS/$EXECUTABLE" --check)"
expected="$(printf '%s\n' "$check" | sed -n 's/^expected bundle identifier: //p')"
[ "$expected" = "$bundle_id" ] || die "bundle identifier $bundle_id differs from the code's APP_ID ($expected)"
printf '%s\n' "$check" | grep -q "^bundle: .*($bundle_id)" || die "the executable does not see its bundle"
if [ "$native" = 1 ]; then
  native_lines="$(printf '%s\n' "$check" | grep '^native: ' || true)"
  printf '%s\n' "$native_lines"
  case "$native_lines" in ""|*"native: "*none*) die "--check reports no loadable native library" ;; esac
fi

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
