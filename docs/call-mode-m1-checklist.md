# Call mode, milestone M1: manual check in a real call

Status: for the owner; written with work package CT-8 (audio-only coordinator).
Source: pipeline-design section 9.4, items 1, 2, 5, 6 and 7 (the items that apply to a client with no translation).

M1 is the client completing the audio loop by itself, with no AI in it: the remote party hears the owner's own voice through the virtual microphone, and the owner hears the remote party as before. Nothing is translated, so pause, resume and the translated output (items 3 and 4) are not part of this check.

## What M1 does and does not do

- **Outgoing:** the physical microphone is read by the client, passed through the mixer (original voice only) and written into the "PRX Voice Microphone" driver. While call mode is on, the system default input is "PRX Voice Microphone", so the call application records that device.
- **Incoming:** the client taps the call application's output, measures it (levels, counters, the silence watchdog) and **plays nothing itself**. The tap is **unmuted**, so the call application keeps playing the remote party on your output exactly as before, and you hear it once, not twice. This is the design's "original only" behaviour (`call.local_output_mode = original`) for a build without translation. A muted tap is not used in M1.
  - Pipeline-design 9.4 item 2 says "(muted tap)"; for M1 the check is "heard once, unmuted" instead, because the client plays nothing on the incoming side yet. The muted-tap arrangement arrives with translated playback (M2/M3) and is checked then.
- Call mode and an assistant session cannot run together. Starting one while the other runs is refused with a message.

## Before you start

1. The microphone driver is installed and current, USB variant (the only variant used in a call so far): `scripts/check-driver.sh` reports it, or the debug view says so when you press Start.
2. A second device (phone) that can take a WhatsApp call from this Mac, and headphones if you can: with speakers the remote party's voice can reach the microphone and echo.
3. Run the app from its bundle, never with `cargo run` (a bare binary gets no consent and the tap delivers silence):

   ```bash
   PRXVC_CODESIGN_IDENTITY="PRX Voice Development" scripts/run-app.sh
   ```

   The log is `~/Library/Application Support/org.openprx.prx-voice-client/logs/prx-voice.log`.
4. Optional: set `call.target_app` in `config.toml` (`~/Library/Application Support/org.openprx.prx-voice-client/config.toml`). Left empty, the control uses `net.whatsapp.WhatsApp`, and the debug view shows the application it uses.
5. Open the debug window: right-click the orb and choose **Debug** (Chinese: **调试**). The section "Call mode (temporary control)" holds the buttons and the readouts used below.
6. Start WhatsApp. Leave it idle (no call yet).

The first Start asks for two permissions, one after the other, as system prompts: **Microphone** and **System Audio Recording**. Allow both. If one was refused earlier, the debug view shows the message with the System Settings path; allow it there and press Start again.

## Item 1: call mode starts from the bundle

1. In the debug view press **Start call mode**.
2. Expected: the state goes to "Audio ready (passthrough)". "Default input" reads "PRX Voice Microphone". The rows show your microphone, the virtual microphone sink, the call application and the call detection ("the application is not recording"). No red issue.
3. Both paths show levels: speak and watch "Outgoing path" level move; play something in WhatsApp (see item 2) and watch "Incoming path".
4. Start a WhatsApp call from the phone to this Mac and answer it. Expected within about a second: call detection reads "a call is using the PRX Voice Microphone". (If it reads "another microphone: ...", WhatsApp chose a different input: in WhatsApp's call settings select "PRX Voice Microphone".)
5. Cross-check, optional: `cargo run -p prxvc-audio --example call_activity -- net.whatsapp.WhatsApp` shows WhatsApp's input device in the same terms.

If Start fails, the debug view says why (not in the bundle, a permission, the driver, no microphone, the call application not running). Write the message down; do not continue.

## Item 2: both parties hear, and the original is not heard twice

During the call:

1. Speak. On the phone you hear your own voice, coming through the PRX Voice Microphone. In the debug view "They hear" reads "the original voice".
2. Speak on the phone. You hear the remote party once, from WhatsApp, as in any call. Expected: no echo, no doubled voice, no delay between WhatsApp's own playback and anything else (the client plays nothing). In the debug view "You hear" reads "the original, played by the call application" and the incoming level moves with the remote voice.
3. Expected counters: "overruns", "drops" and "underruns" stay at or near 0 over a minute of talking.

## Item 5: the default input is changed away and back

1. During the call open System Settings, Sound, Input and select another microphone (for example the built-in one).
2. Expected within about a second: an issue appears in the debug view ("The other person can't hear translations: the system microphone was changed" in M1 wording: the other person now hears the selected microphone, not the PRX path), "Default input" reads "changed to ...", and the button **Use PRX Voice Microphone again** is enabled. The client does not change it back by itself; leave it for 10 seconds to see that.
3. Press **Use PRX Voice Microphone again**. Expected: the issue disappears, "Default input" reads "PRX Voice Microphone" again, System Settings shows it selected, and the remote party hears you through it again.
4. Also check selecting "PRX Voice Microphone" by hand in System Settings clears the issue without the button.

## Item 6: WhatsApp quits and returns; the Mac sleeps

1. Quit WhatsApp during call mode. Expected: an issue "The call application closed; waiting for it." The call state stays "Audio ready". Start WhatsApp again: the issue goes away and the incoming level moves again when it plays sound (the tap follows the application; on macOS earlier than 26 it is re-created within a few seconds).
2. Put the Mac to sleep for one minute (Apple menu, Sleep) and wake it. Expected on wake: the state shows "Recovering", then returns to "Audio ready" within a few seconds; the default input is still "PRX Voice Microphone" (if not, the displacement issue of item 5 appears); both levels move again. If recovery fails for 10 seconds the state becomes "Degraded" with "Audio devices did not come back in time": press **Stop call mode** and Start again, and report it.

## Item 7: stop restores the microphone; a crash is repaired at the next start

1. Press **Stop call mode**. Expected: the state goes to "Idle" and the default input in System Settings is back on the microphone you used before Start (the debug view's last restore names it). WhatsApp's call, if still running, now records that microphone.
2. Start call mode again, then kill the app hard from a terminal:

   ```bash
   pkill -9 -f "PRX Voice.app/Contents/MacOS/prxvc-desktop"
   ```

   The default input stays "PRX Voice Microphone" until the next start (this is the known residual risk: no feeder is writing, so the other party hears silence).
3. Start the app again (`scripts/run-app.sh`). Expected: the default input is back on your microphone before anything else happens, and the debug view's event list contains "RoutingRestored". The routing journal (`routing-journal.json` in the data directory) is gone.
4. Quitting the app normally (tray menu, Quit) while call mode is on must also restore the default input, with no journal left.

## What to report

For each item: pass or fail, the debug view's state and issue text at the moment of a failure, and the last lines of the log. Counters from item 2 after one minute of talking. Anything that sounded wrong (echo, a click when starting or stopping, a gap).

## Not part of M1

Pause, resume, skip, the translated voices, captions, speaker mode and the ducking measurement (checklist items 3, 4 and 8) belong to M2 and M3.
