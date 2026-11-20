# Virtual microphone test plan (owner at the machine)

Goal: find out, with objective numbers where possible, whether the adapted driver
"PRX Voice Microphone" works as the input of WhatsApp Desktop and Telegram, and how it
compares with BlackHole 2ch. Nothing here had been run against an installed driver when this plan was written.
Results recorded 2026-11-20: the virtual and then the USB variant were installed; the USB variant passed the
loopback self-test and a WhatsApp call (section 7 results table). USB is the variant to ship.

All paths are relative to the repository root. `B` below means `tools/macos/build/bin/prx-vmic`.

## 0. Prepare (no admin)

```sh
scripts/build-macos-tools.sh                      # prx-vmic and the host-side load test
driver/macos/build.sh --transport virtual         # one variant (default: usb) into driver/macos/build/<variant>/
driver/macos/build.sh --transport all             # all variants (virtual usb builtin other); see section 7
scripts/check-driver.sh                           # builds all variants; expect "PASSED (0 failures)" for each
driver/macos/status.sh                 # expect: "bundle : not installed", "device : not present"
tools/macos/build/bin/prx-vmic tone --seconds 1   # expect: error ... not installed, exit code 2
```

## 1. Install (admin)

```sh
sudo driver/macos/install.sh
```

It prints its plan, asks for confirmation, copies the bundle to
`/Library/Audio/Plug-Ins/HAL/PrxVoiceMicrophone.driver`, restarts coreaudiod (audio drops for a few
seconds), and lists the device.

Observe:
- install.sh ends with `OK: PRX Voice Microphone is present` -> the system loaded an ad-hoc signed driver.
- It ends with `WARNING ... did not appear` -> signing is probably the cause. Record the output of
  `codesign -dv --verbose=4 /Library/Audio/Plug-Ins/HAL/PrxVoiceMicrophone.driver` and
  `log show --last 5m --predicate 'process == "coreaudiod"' | grep -i -E 'prx|plug|sign|reject'`,
  then stop and report; the next step would be a Developer ID signature.
- `driver/macos/status.sh` shows version `0.1.0-virtual` (a bundle installed before variants existed shows `0.1.0`) and `device : PRESENT`.
- `tools/macos/build/bin/prx-vmic list` shows `PRX Voice Microphone` (1 input channel) and the hidden sink line
  `(hidden sink present ...)`.

## 2. Loopback self-test without any call app

The first run opens an input stream: macOS asks for **microphone permission for the terminal app**
(Terminal, iTerm, ...) that launches the command. Allow it. If you denied it earlier, enable it in
System Settings > Privacy & Security > Microphone. Nothing is played on the speakers and the real
microphone is not used (the recorder reads the PRX device).

```sh
tools/macos/build/bin/prx-vmic selftest --out /tmp/prx-loopback.wav
```

The tool plays 1 s silence, 2 s tone (440 Hz, amplitude 0.5), 1 s silence and a 1 ms click into the
hidden sink while recording the virtual microphone.

Pass criteria (the tool prints `RESULT: PASS` when all hold):

| Check | Expected |
|---|---|
| sink / mic format | 48000 Hz, 1 ch, float32 |
| dominant frequency | 440 +/- 1 Hz |
| tone RMS | 0.3536 (-9.0 dBFS) within 1.5 dB |
| dropout blocks inside the tone | 0 |
| tone length | about 2.00 s |
| click latency (write into sink -> read from mic) | below 100 ms; the driver caps its queue at 768 frames (16 ms), so a few tens of ms is expected |
| feeder underruns / overruns | 0 / 0 (small numbers are scheduling noise, note them) |

Record the printed lines. Run it three times; also try one run while the Mac is busy (for example a
video playing in a browser) and note whether underruns or dropouts appear.

Separate tools, for manual checks:

```sh
tools/macos/build/bin/prx-vmic tone --seconds 10                      # feeder only (for the call tests below)
tools/macos/build/bin/prx-vmic play path/to/speech.wav                # WAV file, resampled to 48 kHz mono if needed
tools/macos/build/bin/prx-vmic record --seconds 5 --out /tmp/x.wav    # recorder only
tools/macos/build/bin/prx-vmic analyze /tmp/x.wav                     # peak, RMS, dominant frequency
```

## 3. WhatsApp Desktop call test

Needs a second device (phone) with its own WhatsApp account to receive the call. Use headphones on
the second device or put it in another room so the remote side does not hear your room.

1. Quit WhatsApp Desktop completely. Start it. In its settings choose the microphone
   (Settings > Voice & Video, wording may differ by version): `PRX Voice Microphone`.
   Also allow the Microphone permission for WhatsApp if macOS asks.
2. Start a voice call to the second device and answer it there.
3. In a terminal: `tools/macos/build/bin/prx-vmic tone --seconds 20`. On the second device listen for the tone.
4. Then `tools/macos/build/bin/prx-vmic play path/to/speech.wav` with a clean speech recording (10-20 s,
   one voice, known text).
5. Answer on the second device:

| Observation | Tone | Speech |
|---|---|---|
| Heard at all? | | |
| Pitch right (tone sounds like 440 Hz, voice not chipmunk or slow)? | | |
| Speed right (duration matches)? | | |
| Clear, or clicks / dropouts / robotic artefacts? | | |
| Volume sensible (not very quiet, not clipping)? | | |
| Words cut off at the start of sentences (noise suppression gating)? | | |

6. Noise suppression and echo cancellation: in the call app or in macOS Control Center (menu
   bar > Mic Mode) check whether "Voice Isolation" or "Standard" mic mode is available for the app;
   try each mode and note whether synthetic speech gets quieter, gated or distorted. A steady tone
   that fades out after a few seconds means the app's suppressor treats it as noise; report it.
7. Restart tests, one at a time, each with the same two observations (does the remote side hear audio
   again, and what did you have to do):
   - Quit and restart WhatsApp during a call (or end the call, restart WhatsApp, call again) with the
     feeder still running (`tone --seconds 120`).
   - Stop the feeder (Ctrl-C) mid-call, wait 10 s, start it again.
   - Sleep and wake: while a call-less WhatsApp has the PRX microphone selected, close the lid or
     choose Apple menu > Sleep for 30 s, wake, then `driver/macos/status.sh`, `tools/macos/build/bin/prx-vmic tone --seconds 5`, and a
     new call. Note whether the device is still present and whether WhatsApp kept its selection.

## 4. Telegram Desktop call test

Repeat section 3 with Telegram Desktop: select `PRX Voice Microphone` in its call settings
(Settings > Advanced > Voice and Video, wording may differ), call the second device, play the tone and
the speech file, fill in the same table and repeat the restart and sleep checks.

## 5. Optional comparison with BlackHole 2ch

Install it yourself (admin, may need a reboot; GPL source, the official binary is not
redistributable, so this is for comparison only):

```sh
brew install blackhole-2ch
```

Then repeat sections 2-4 with BlackHole as the destination. BlackHole is one device for both
directions, so the feeder and the call app both point at "BlackHole 2ch":

```sh
tools/macos/build/bin/prx-vmic list                                   # find the exact name and UID
tools/macos/build/bin/prx-vmic selftest --sink "BlackHole 2ch" --mic "BlackHole 2ch" --out /tmp/bh-loopback.wav
tools/macos/build/bin/prx-vmic tone --seconds 20 --device "BlackHole 2ch"
tools/macos/build/bin/prx-vmic play speech.wav --device "BlackHole 2ch"
```

In the call apps select `BlackHole 2ch` as the microphone. The feeder resamples to the BlackHole
rate automatically and writes the mono signal to both channels. Fill in the same tables and compare:
latency, dropouts, how the apps' noise suppression treats the signal, restart and sleep behaviour,
and the install effort (admin, reboot). Remove it afterwards with `brew uninstall blackhole-2ch`
(admin; may need a reboot).

## 6. Uninstall

```sh
sudo driver/macos/uninstall.sh
driver/macos/status.sh        # expect: bundle not installed, device not present
```

Select another microphone in WhatsApp and Telegram afterwards (they fall back to the default on their own,
but check).

## 7. Call test with transport variants

Why: WhatsApp's call device list skips devices whose transport type is Virtual or Aggregate (static
analysis, `bench/callapp/REPORT.md` section 5). Voice messages already work with the Virtual variant;
whether a call falls back to the built-in microphone is unknown. The driver can report a different
transport type, chosen at build time. Only one variant is installed at a time.

| Variant | Transport type reported | Rationale |
|---|---|---|
| `virtual` | `virt` | Installed first (voice messages work with it). The baseline; never tried in a call. |
| `usb` | `usb ` | In WhatsApp's keep list; most generic external microphones. Try first. |
| `builtin` | `bltn` | In the keep list; macOS and some apps special-case it. Try second. |
| `other` | `pci ` (PCI) | Non-zero and in no skip list; WhatsApp treats unlisted values as "unknown but kept". Tests the unknown path. |

Build once (no admin): `driver/macos/build.sh --transport all`. Each variant ends with `PASSED (0 failures)`.

Order: `virtual` (already installed; run the call once for the baseline), then `usb`, then `builtin`,
then `other`. Stop at the first variant for which the call works end to end, and record the rest only if
wanted.

Per variant V (for `virtual` skip the install line):

```sh
sudo driver/macos/install.sh --transport V       # V = usb | builtin | other ; prints the variant it installs and the current one
driver/macos/status.sh                           # expect: variant : V, and live : transport=<fourcc> variant=V
tools/macos/build/bin/prx-vmic selftest           # loopback check; PASS expected (needs no other prx-vmic player running)
```

After the install, check in System Settings > Sound > Input that "PRX Voice Microphone" is still the
selected default input (a re-install may reset it); select it if not.

Call (second device with its own WhatsApp account, headphones or another room):

1. Quit and restart WhatsApp Desktop (it builds its device list at start and on device changes).
2. Start a voice call to the second device and answer it.
3. Feed speech: `tools/macos/build/bin/prx-vmic play path/to/speech.wav` (repeat as needed; a 10-20 s clean speech file).
4. While the speech plays, in another terminal run
   `bench/callapp/procaudio (voice workspace, not in this repository) -f whatsapp -w 1` and read the WhatsApp row: which
   input device the process has open (expected `PRX Voice Microphone`; `MacBook Pro Microphone` means
   the call fell back to the built-in microphone).
5. In the call window open the Microphone menu: is "PRX Voice Microphone" listed, and which entry is ticked.
6. Ask the other side what they hear (nothing, room noise, the speech clear, garbled).

Results (fill in; one row per variant tried):

| Variant | status.sh live transport | Input device WhatsApp opened (procaudio) | In the in-call Microphone menu? | Other side hears | Notes (selftest result, artefacts) |
|---|---|---|---|---|---|
| virtual | virt | not tried in a call (voice message only: WhatsApp recorded from it, input running about 12 s at 15:59:15) | not checked | not tried in a call; on voice-message playback the owner heard the injected speech | installed first; self-test tone, RMS and dropouts passed, click latency invalid (a second player wrote into the sink) |
| usb | usb | **PRX Voice Microphone** (call 16:08:04 to 16:08:50, WhatsApp PID 23453; output on MacBook Pro Speakers) | not checked | **the synthesized Mandarin speech fed into the hidden sink** | version `0.1.0-usb`, ad-hoc signed; set as system default input; `selftest` PASS, 14.1 ms end to end |
| builtin | bltn | not tried | | | built only, not installed |
| other | pci | not tried | | | built only, not installed |

Outcome (2026-11-20): the USB variant is the countermeasure and the variant to ship. Following the stop rule
above, the other variants were not tried in a call; the virtual variant was skipped for the call because the owner
installed the USB variant directly, so whether WhatsApp's call path would open a Virtual-transport default device
remains unknown. Details: `bench/callapp/REPORT.md`, section "Live call results".

Interpretation: input on PRX and speech heard -> that variant is the countermeasure. Input on the
built-in microphone for every variant -> the filter is not the (only) cause; capture `procaudio` output and
`log show --last 5m --predicate 'process == "WhatsApp"'` and report. Input on PRX but garbled or silent
speech -> voice processing, not the device list (callapp REPORT section 8).

Restore after the test: `sudo driver/macos/install.sh --transport virtual` (or `sudo driver/macos/uninstall.sh`), then
`driver/macos/status.sh`. (Since the USB variant is the one to ship, keep it installed rather than restoring the virtual one.) If a variant leaves audio in a bad state, `sudo driver/macos/uninstall.sh` removes the driver.

## Result sheet

Send back: the install.sh output, the selftest output (three runs), the filled tables for WhatsApp,
Telegram and (optionally) BlackHole, and the macOS version (`sw_vers`).
