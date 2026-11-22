# PRX Voice Microphone driver (macOS)

A virtual microphone for call applications. The driver is a Core Audio
AudioServerPlugIn (a HAL plug-in loaded by the audio daemon, `coreaudiod`). It exposes two
devices:

| Device | UID | Visible | Role |
|---|---|---|---|
| PRX Voice Microphone | `org.openprx.prx-voice-client.microphone` | yes | 48 kHz mono float32 input device that call apps open as their microphone |
| PRX Voice Microphone (Internal Sink) | `org.openprx.prx-voice-client.microphone.sink` | no | 48 kHz mono float32 output device; whatever the client writes here comes out of the microphone |

A bounded ring (16384 frames) joins the two; after a burst the reader skips to the recent
audio, so latency stays capped (about 16 ms plus one block). The bundle identifier is
`org.openprx.prx-voice-client.microphone-driver`.

The source is one C file, `PrxVoiceMicrophoneDriver.c`, adapted from the MIT-licensed Hex
microphone driver. The original copyright and permission notices are kept at the top of the
file and in `NOTICE` (also copied into the bundle at `Contents/Resources/NOTICE` and into
`THIRD_PARTY_NOTICES.md`). Do not remove them.

## Files

| File | What |
|---|---|
| `PrxVoiceMicrophoneDriver.c` | the driver |
| `Info.plist.in` | bundle Info.plist template (`@VERSION@`, `@BUILD_NUMBER@`) |
| `VERSION` | the driver version, `X.Y.Z`; the single source (see Version) |
| `NOTICE` | licence and attribution of the adapted source |
| `build.sh` | builds one or all variants, signs, runs the structural checks |
| `install.sh`, `uninstall.sh`, `status.sh` | developer scripts (admin rights for the first two) |

Test tooling lives in `tools/macos/` (`prx-vmic`, the host-side load test) and is built by
`scripts/build-macos-tools.sh`; the repository check is `scripts/check-driver.sh`.

## Variants and why `usb` is the default

The driver reports a transport type for both devices, chosen at build time
(`-DPRX_TRANSPORT_TYPE`, selected with `build.sh --transport`):

| Variant | Transport type | Note |
|---|---|---|
| `virtual` | `kAudioDeviceTransportTypeVirtual` (`virt`) | the original behaviour |
| `usb` | `kAudioDeviceTransportTypeUSB` (`usb `) | **default, the one to ship** |
| `builtin` | `kAudioDeviceTransportTypeBuiltIn` (`bltn`) | built, never installed; the riskiest for side effects |
| `other` | `kAudioDeviceTransportTypePCI` (`pci `) | built, never installed; non-zero and in no skip list |

Why `usb` ships: call applications skip Virtual and Aggregate transport types in their device lists.
The `usb` variant was verified in a WhatsApp Desktop call on macOS 26.6 (Apple Silicon); the other
variants are built but untested.

Only one variant is installed at a time (same bundle id and device UIDs); installing replaces the
current one. Bundles record their variant in `PrxTransportVariant` and `PrxTransportType`
(Info.plist) and in the version string `X.Y.Z-<variant>`.

## Version

`driver/macos/VERSION` holds `X.Y.Z`. `build.sh` writes `CFBundleShortVersionString` as
`X.Y.Z-<variant>` and `CFBundleVersion` as `X*10000 + Y*100 + Z` (the app's rule). The app
reads the version and variant from the bundled driver's Info.plist at run time and compares
them with the installed ones (`apps/desktop/src/driver_install/`), so no second constant is
needed for the install decision. A code-side constant for "the driver version the client
expects" (`crates/audio`, driver status work) has to be derived from this same file, for
example with a `build.rs` that reads `driver/macos/VERSION` or a test that asserts equality.

## Build

```sh
driver/macos/build.sh                         # usb variant -> driver/macos/build/usb/PrxVoiceMicrophone.driver
driver/macos/build.sh --transport all         # all four variants
driver/macos/build.sh --transport virtual --out /some/dir
```

Needs the Command Line Tools only; arm64 only (add `-arch x86_64` in `build.sh` to widen).
`build.sh` compiles with `-Wall -Wextra -Werror`, signs the bundle, and checks: `plutil -lint`,
`codesign --verify --strict`, the architecture, the factory symbol
`_PrxVoiceMicrophoneDriverCreate`, and the variant key. `build/` is git-ignored.

The host-side load test (`scripts/check-driver.sh`, also run by `scripts/check.sh` on macOS)
builds every variant and loads each binary with `dlopen` without the audio daemon: it asserts
the factory UUID handling, the bundle id, names and manufacturer, the two device UIDs (and that
the original reference's UID is not served), the transport value of the variant on both
devices, the format, a ramp written into the sink read back from the microphone with zero
discontinuities, and the latency cap. A usb binary tested against `virt` must fail; the script
checks that too.

## Install and uninstall

By script (developer machines; needs admin rights, asks before acting, restarts the audio
daemon so all audio drops out for a few seconds):

```sh
driver/macos/build.sh
sudo driver/macos/install.sh             # usb; --transport V for another built variant; -y skips the question
driver/macos/status.sh                   # bundle, variant, version, signature, live properties
sudo driver/macos/uninstall.sh
```

From the app: `scripts/package-macos.sh` (and `scripts/run-app.sh`) build the usb variant and
place it at `PRX Voice.app/Contents/Resources/Drivers/PrxVoiceMicrophone.driver` with its
notice. The app's UI commands `RefreshDriverStatus`, `InstallDriver` and `UninstallDriver`
(`apps/desktop/src/driver_install/`) compare bundled and installed driver, then install, update
or remove it through the system administrator password prompt (`osascript` with administrator
privileges). The privileged shell command is fixed except for the bundle path, which is
validated and single-quoted; the staged copy is verified as root (identifier, signature) before it
replaces the installed driver; then the audio daemon is restarted exactly as `install.sh` does and
the app waits for the device to appear. There is no UI for it yet.

## Self-test

```sh
tools/macos/build/bin/prx-vmic list        # PRX Voice Microphone and the hidden sink line
tools/macos/build/bin/prx-vmic props       # live transport=usb variant=usb, hidden, alive
tools/macos/build/bin/prx-vmic selftest    # tone and click through sink -> driver -> microphone; PASS expected
```

`selftest` writes into the silent hidden sink and records only from the virtual microphone. The
first run asks for microphone permission for the terminal application. Run it only when no other
process feeds the sink (another player pollutes the click-latency check).

## Signing

- Development: `PRXVC_CODESIGN_IDENTITY` selects the identity, exactly as for the app
  (`scripts/package-macos.sh`); unset means ad-hoc (`codesign --sign -`). With an identity the
  hardened runtime and a timestamp are used. An ad-hoc signed driver is loaded by `coreaudiod`
  on the developer machine (macOS 26.6, observed with both the virtual and the usb variant).
- The same identity signs the driver inside the app, before the outer bundle is signed.
- Distribution needs more: a Developer ID Application signature on the driver and the app,
  notarization of the app and of any installer package, and stapling (pipeline-design PD-8.1.3,
  PD-8.2.3). A driver inside a downloaded app carries the quarantine attribute and needs that to
  pass Gatekeeper. With a Developer ID build, the install step should also pin the signing team
  (`install_command` accepts a Team ID and then requires
  `anchor apple generic and certificate leaf[subject.OU]`); the team identifier is not configured yet.
- A hardened-runtime, Developer ID signed driver has not been tried on this machine (no such
  identity here); the first such build must be checked by loading it.
