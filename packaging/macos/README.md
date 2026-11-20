# macOS packaging

`scripts/package-macos.sh` builds `PRX Voice.app` from `prxvc-desktop`. No Xcode
project: plain shell plus `cargo`, `plutil`, `codesign`, `iconutil`, `sips` and
`ditto` from the Command Line Tools.

```sh
scripts/package-macos.sh                 # release build -> dist/PRX Voice.app
scripts/package-macos.sh --zip           # also dist/PRX-Voice-<version>-<arch>.zip
scripts/package-macos.sh --target x86_64-apple-darwin
scripts/run-app.sh                       # debug bundle (dist/debug/), ad-hoc signed, opened
scripts/run-app.sh --no-open             # build and verify only
```

`dist/` is git-ignored.

## Layout

```text
PRX Voice.app/Contents/
  Info.plist                      from packaging/macos/Info.plist.in
  PkgInfo
  MacOS/prxvc-desktop             the release (or debug) binary
  Resources/
    AppIcon.icns                  from packaging/macos/icon/AppIcon-1024.png
    en.lproj/InfoPlist.strings    from packaging/macos/resources/
    zh-Hans.lproj/InfoPlist.strings
    LICENSE, LICENSE-MIT, LICENSE-APACHE, THIRD_PARTY_NOTICES.md
  _CodeSignature/
```

Native libraries that the app will load later belong in `Contents/Frameworks/`;
the script signs everything found there (and in `PlugIns/`, `Library/`) before it
signs the bundle.

## Single sources of truth

- Version: `cargo pkgid -p prxvc-desktop` (the workspace package version).
  `CFBundleShortVersionString` is `X.Y.Z` (a prerelease suffix is dropped);
  `CFBundleVersion` is `X*10000 + Y*100 + Z` (0.1.0 is 100).
- Bundle identifier: `APP_ID` in `crates/config/src/paths.rs`, which also names the
  data directory. The script reads that constant, and after building it runs the
  bundled executable with `--check` and fails if the executable disagrees. The
  identifier is `org.openprx.prx-voice-client` and must never change after a release:
  every user would have to grant permissions again.

## Info.plist decisions

| Key | Value | Reason |
|---|---|---|
| `LSMinimumSystemVersion` | 14.4 | Core Audio process taps need 14.2; the tap shim (`crates/audio/native/prxvc_tap.m`) is written against 14.4 (`@available(macOS 14.4, *)`, per-process audio objects). No other API in the app needs more. |
| `LSUIElement` | true | See below. |
| `NSMicrophoneUsageDescription` | purpose string | Microphone capture (cpal). Localized in `InfoPlist.strings`. |
| `NSAudioCaptureUsageDescription` | purpose string | Without it a process tap delivers exact digital zeros with no error. |
| `NSHighResolutionCapable` | true | Retina rendering. |
| `LSApplicationCategoryType` | `public.app-category.productivity` | |
| `CFBundleLocalizations` | `en`, `zh-Hans` | Matches the UI languages. |

No other usage keys: the app uses no camera, screen capture, contacts, calendar,
Apple events or accessibility API (hotkeys use Carbon `RegisterEventHotKey`).

### Activation policy: `LSUIElement` = true

The app is a small frameless always-on-top orb (`apps/desktop/src/main.rs`:
`with_decorations(false)`, `with_always_on_top`), a tray icon
(`apps/desktop/src/tray.rs`, with Show, Start, Stop, Settings and Quit) and tool
windows. It is a status surface that stays running while the user works in a call
application. A Dock icon and a Cmd-Tab entry for it would be noise, and the tray
menu is the way back to every window and the only place to quit.
winit 0.30 leaves the activation policy alone when the process is bundled
(`app_state.rs`: "do not set the activation policy at all, to allow the package
manifest to define the behavior via LSUIElement"), so the key takes effect without
code. When the binary is started bare (`cargo run`) winit sets the policy to
Regular: a Dock icon appears, which is the right behaviour for an unbundled
development run and a visible hint that it is not the bundle.

Consequences to check by hand (not done here: the GUI was not launched): the
settings window must come to the front when opened from the tray, and Cmd+Q does
nothing (no menu bar; Quit is in the tray). If the settings window turns out to open
behind other applications, the fix is to activate the application when the window
is opened, not to drop `LSUIElement`.

## Icon

`icon/AppIcon-1024.png` is a **PLACEHOLDER**: a rounded square in the theme's
primary colour (`#222224`) with a white hexagon outline, the shape of the tray icon.
It is original and drawn by `icon/make-placeholder-icon.py` (standard library only;
`python3 make-placeholder-icon.py AppIcon-1024.png`). Replace the PNG with real
artwork when it exists; the script only needs a 1024 by 1024 PNG with alpha.

## Signing

`PRXVC_CODESIGN_IDENTITY` selects the identity.

- Unset: ad-hoc (`codesign --force --sign -`), no hardened runtime. This is what
  development and CI without a certificate use. The entitlements file is applied too
  (the audio-input entitlement is harmless without the hardened runtime).
- Set (Developer ID Application): `--timestamp --options runtime` with
  `entitlements.plist`, nested code first, then the bundle.

Ad-hoc builds must not use `--options runtime`: under the hardened runtime, library
validation only loads libraries signed by Apple or with the same Team ID, and an
ad-hoc signature has no team. Checked on this machine: the ad-hoc bundle verifies
(`codesign --verify --deep --strict`) and its executable runs from inside the bundle.

### Entitlements (`entitlements.plist`)

- `com.apple.security.device.audio-input`: required for microphone access once the
  hardened runtime is on (Apple, "Hardened Runtime" and "Audio Input Entitlement");
  the reference application (`reference/hex`) ships the same one.
- Process taps (`AudioHardwareCreateProcessTap`): no entitlement. Apple's documentation
  for taps ("Capturing system audio with Core Audio taps", as recalled; not
  re-checked online in this work) names only
  `NSAudioCaptureUsageDescription` and the user's consent; the app is not sandboxed,
  so no sandbox entitlement applies either. The reference application records
  meeting audio the same way with only the usage key and `audio-input`. This was not
  tested under the hardened runtime here (no Developer ID identity on this machine);
  the first Developer ID build must be checked with a real tap.
- `com.apple.security.cs.disable-library-validation`: **not included, and avoidable.**
  Libraries loaded later (model runtimes) are put in `Contents/Frameworks/` and signed
  by the script with the same identity, so library validation passes. The entitlement
  is needed only to `dlopen` a library signed by someone else or lying outside the
  bundle (for example downloaded into the data directory); do not do that. If a
  third-party binary must be loaded unchanged, re-sign it with our identity.
- `allow-jit`, `allow-unsigned-executable-memory`: not included; nothing in the app
  generates code at run time as far as the sources show. Confirm with the first
  hardened-runtime build.

### Notarization (separate step, not run here)

Needs a paid Apple Developer account, a Developer ID Application certificate in the
keychain, and credentials for `notarytool`, either a stored profile
(`xcrun notarytool store-credentials PRXVC --apple-id ... --team-id ... --password <app-specific password>`)
or an App Store Connect API key (`.p8`, key id, issuer id). The script runs it
only when credentials are present:

```sh
PRXVC_CODESIGN_IDENTITY="Developer ID Application: Name (TEAMID)" \
PRXVC_NOTARY_PROFILE=PRXVC \
scripts/package-macos.sh --zip
# or: PRXVC_NOTARY_KEY=/path/AuthKey.p8 PRXVC_NOTARY_KEY_ID=... PRXVC_NOTARY_ISSUER=...
```

It zips the bundle with `ditto`, runs `xcrun notarytool submit --wait`, then
`xcrun stapler staple` and `spctl --assess`. Notary credentials without a Developer
ID identity are refused. `--zip` runs after stapling, so the zip carries the ticket.

## Permissions and development

macOS keeps microphone and audio-capture consent (TCC) per **bundle identifier and
code signature**, not per path. Consequences:

- Run the app from the bundle (`scripts/run-app.sh`). A bare binary started from a
  terminal has its consent attributed to the terminal application, the prompt carries
  the terminal's name, and a Core Audio tap delivers exact zeros with no error and no
  prompt. At start the app logs one warning when it is not in a bundle, and
  `SharedAppState` carries the flag for the permissions view
  (`UiSnapshot::outside_app_bundle`).
- **An ad-hoc signature has no identity, only a hash.** The designated requirement
  of the ad-hoc bundle is `cdhash H"..."`, the hash of the code. Measured here: the
  release bundle has `cdhash H"6f06ab4d..."`, a debug bundle of the same source has
  `H"713efbaf..."`; a rebuild after any code change gives a new hash, and re-packaging
  the same binary gives the same hash. TCC stores the requirement, so after a rebuild
  the old consent no longer matches the new binary: expect the prompts to appear
  again (and a stale entry for the old build can remain in System Settings). This
  was derived from the signature, not observed through a prompt, because prompts must
  not be triggered during packaging work; the previous measurement that consent
  survives relaunches (`bench/callapp`) is for an unchanged binary.
- **Keeping consent across rebuilds**: sign development builds with one stable
  identity, so the designated requirement is `identifier "org..." and certificate
  leaf = H"<cert hash>"` instead of a code hash. A self-signed local certificate
  is enough (steps, not performed here):
  1. Keychain Access, menu Certificate Assistant, Create a Certificate: name
     `PRX Voice Development`, identity type Self Signed Root, certificate type Code
     Signing. (Command-line alternative: create the certificate with `openssl`
     with `extendedKeyUsage = codeSigning`, then
     `security import cert.p12 -k ~/Library/Keychains/login.keychain-db`.)
  2. Trust it for code signing: open the certificate, Trust, Code Signing: Always
     Trust (or `security add-trusted-cert -p codeSign <cert.pem>`).
  3. Check with `security find-identity -v -p codesigning`.
  4. Build with `PRXVC_CODESIGN_IDENTITY="PRX Voice Development" scripts/run-app.sh`.
     This turns on the hardened runtime in the script (the identity is not `-`), which is
     fine for a self-signed certificate as long as all nested code is signed with it too.
  Re-grant once after the first build with the new signature; later rebuilds keep it.
- Reset the app's consent (for a stale entry or a clean test):

  ```sh
  tccutil reset Microphone org.openprx.prx-voice-client
  tccutil reset AudioCapture org.openprx.prx-voice-client
  # everything for the app:
  tccutil reset All org.openprx.prx-voice-client
  ```

  (`AudioCapture` is the TCC service behind system audio recording. Both service
  names were run against the bundle identifier on macOS 26 and reported
  "Successfully reset"; nothing had been granted yet.)

## Running from the bundle: what was checked

`prxvc-desktop --check` (no window, no directory created, no credential or audio
device access) run from inside the bundle with the working directory `/` prints the
bundle, resources, config, data, models, voices and log paths. All of them come from
`prxvc_config::paths::AppPaths` or from the executable path; fonts and the tray icon
are compiled into the binary (`include_bytes!`, generated pixels), and the CJK system
fonts are absolute paths. Nothing in the app reads a path relative to the repository.

Logging: in a terminal the log goes to stderr; otherwise (Finder, `open`) to
`<data dir>/logs/prx-voice.log`, mode 0600, rotated at 5 MiB with three backups.
`RUST_LOG` selects the filter in both cases (default `info`). Recognised and
translated text is logged only at `debug`; `info` lines carry character counts.
