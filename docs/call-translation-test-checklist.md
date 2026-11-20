# Translated call: first real test (milestones M2 and M3)

Status: for the owner; written with the work package that composes both directions (CT-15, CT-16 and the minimum of CT-17).
Source: pipeline-design sections 4.5, 4.6, 5.4, 6 and 9.4 (items 3, 4 and 8), and `docs/call-mode-m1-checklist.md` for the audio-only items that still apply.

What this test shows: you press **Start** with **Translate** on, make a WhatsApp call, and both directions are translated in the client. The other person hears your speech translated and spoken in your cloned voice. You hear the other person translated, spoken in a stand-in voice. Nothing here was run in a real call before; everything below that needs a device, WhatsApp or a permission prompt is yours.

## What the build does

- **Outgoing** (you speak Mandarin, the other person hears English): microphone, voice detection, recognition (a streaming draft, then a second decode of each clause), translation by the local server, synthesis in your cloned voice (embedding-only mode), the mixer, the virtual microphone. What the other person hears is decided by the direction's state machine and announced in the debug view: the translation, your own voice (while paused or after a failure, the default), or nothing.
- **Incoming** (the other person speaks English, you hear Chinese): the client taps WhatsApp **muted while tapped**, so you do not hear the original twice and WhatsApp comes back by itself if the client dies. The client plays to your output through its own mixer: first the original (so you are never left in silence), then the translation per `call.local_output_mode` (default `translation`: translation only; `both`: the original lowered by `call.original_gain_db`, -12 dB, under the translation; `original`: the original only). The voice is the preset named in `voice.incoming_preset`.
- **Audio only** (the Translate switch off) is unchanged: unmuted tap, nothing played by the client.
- **Headphones are required.** On the built-in speakers the client shows a warning (the output is the built-in speaker) and, with `audio.echo_mode = auto`, closes the microphone while it plays ("Paused while you listen", PD-6.3).
- **The translation server is yours to start** (decision D-P6).
- **The Skip command exists in the coordinator but has no button yet**; Pause and Resume pause both directions.

## Before you start (development set-up)

1. Headphones on, and selected as the macOS output (System Settings, Sound, Output). Not the built-in speakers.
2. Once, or after changing models or voices (all safe to repeat; details in `scripts/dev/README.md`):

   ```bash
   scripts/dev/adopt-models.sh      # models into the store, checked against the catalogue pins
   scripts/dev/install-voices.sh    # your voice (owner-zh, owner-en), the stand-in preset, config keys
   ```

   The preset `dev-standin` is a **stand-in made with the system synthesizer**, not a product voice and not your voice.
3. Start the translation server and leave it running:

   ```bash
   scripts/dev/llama-server.sh start      # Gemma 4 E4B on http://127.0.0.1:8080/v1, two slots
   ```

4. Build the bundle with both native runtimes, signed with the development identity, without opening it, and check everything:

   ```bash
   PRXVC_CODESIGN_IDENTITY="PRX Voice Development" scripts/run-app.sh --native --no-open
   scripts/dev/preflight.sh
   ```

   Expected: only the two items that need you remain: the audio-capture permission (asked at the first Start) and, if you are on speakers, the output line. Fix any other `MISSING` line before going on; each says what to do.
5. Optional settings in `config.toml` (`~/Library/Application Support/org.openprx.prx-voice-client/config.toml`), applied at the next Start:

   | Key | Meaning |
   |---|---|
   | `call.self_language = "zh"`, `call.peer_language = "en"` | **set these explicitly**: left empty they follow the interface language (`ui.language`), which changes when the language is switched in the app |
   | `call.translate = true` | the Translate switch starts on |
   | `call.output_gain_db` | gain of the translation you hear, dB; raise it if WhatsApp's voice processing lowers it (see item 8) |
   | `call.local_output_mode` | `translation`, `both` or `original` (what you hear) |
   | `debug.call_trace = true` | write one line per clause to `traces/call-<time>.jsonl` (also `PRXVC_CALL_TRACE=1`); add `debug.call_trace_text = true` (or `PRXVC_CALL_TRACE_TEXT=1`) to include the texts. Off by default; nothing is written to disk otherwise. Captions are only on screen |
   | `pipeline.clause_pause_ms`, `pipeline.commit_on_stable_partial` | when a clause is committed (see "What the harness found") |
   | `pipeline.max_backlog_ms` | unplayed audio per direction above which the oldest clause is skipped; see "What the harness found" |

6. Open the app and the debug window: `open -n "dist/debug/PRX Voice.app"`, then right-click the orb and choose **Debug** (Chinese: **调试**). The section "Call mode (temporary control)" holds the switch, the buttons and the readouts below. Start WhatsApp and leave it idle.

The first Start asks for **Microphone** and **System Audio Recording**; allow both.

## Item 1: start translated

1. Turn **Translate** on, press **Start call mode**. Loading takes several seconds (models, then the voice): the state shows "Audio ready (passthrough)" first, so the other person hears your own voice meanwhile, then "Translating".
2. Expected: both lanes show "translating" with their languages (zh to en outgoing, en to zh incoming), "They hear: the translation", "You hear: the translation", no red issue.
3. If a problem is listed instead (state "Degraded", a yellow line per problem, each with what to do): fix it and Stop and Start again. While degraded, both sides hear the original (decision D-P3).

## Item 2: the original is not heard twice

Start a WhatsApp call from the phone to this Mac and answer it. Play any sound from WhatsApp (the other person speaking). Expected: you hear **one** voice, never WhatsApp's original together with the client's playback; with `call.local_output_mode = both` you hear the original lowered under the translation, by design. Say a few words yourself and check on the phone that you do not hear your own original voice together with the translation.

## Item 3: what each side should hear

Say these sentences on your side (Mandarin), one at a time, with a pause between. The other person should hear them in English in **your** voice:

1. 你好，今天下午三点我们开会，请不要迟到。 (hello, meeting at 15:00, do not be late)
2. 我的电话是一三八零零一二三四五六，不是一三九开头的那个。 (a phone number, with a negation)
3. 王建国会在十二月二日乘坐三十七号航班到达。 (a name, a date, a flight number)
4. 这个价格不包括税，一共是一万五千元。 (negation, an amount)
5. 我想说，我们不能同意这个条件。 (do not-agree: negation must survive)

On the phone, say in English, one at a time (you should hear Chinese in the stand-in voice):

1. I will call you at 4:15, please do not hang up before that.
2. The meeting moved to March 3rd, and it is not in the main office.
3. Mr. Smith said the flight number is 37 and the fee is fifteen thousand dollars.

Expected: the translation keeps numbers, dates, names and negations (the prototype's Gemma model once garbled a name, once read "flight 37" as a date and once dropped a phone number; note any such error), no answering or commenting instead of translating, no endless repetition (a looping translation is cut and counted as "suspect"). The first translated audio arrives about 2.5 s after the speech starts when you pause between clauses, and after the end of the sentence when you do not.

## Item 4: pause, resume, both speaking at once

1. Press **Pause**: both listeners hear the original (the default `call.*_when_paused`), the lanes say so, no translation starts. **Resume**: the translation returns. (Skip has no button yet.)
2. Both of you speak at once for a few seconds: both directions keep working and neither blocks the other; note any underruns.
3. Keep talking without pauses for 10 seconds: a long clause may be skipped to keep up (see below); the counters show it.

## Item 5: failure announced, the original plays

1. Stop the translation server (`scripts/dev/llama-server.sh stop`) in the middle of a call, then speak. Expected: after a failed clause the other person hears your original voice, the lane says "the other person hears: your own voice", a failure shows in the issues, and the translation returns by itself a few seconds after `scripts/dev/llama-server.sh start`.
2. Switch the default input away and back as in item 5 of the M1 checklist.

## Item 6: WhatsApp quits, sleep, stop, crash

As items 6 and 7 of `docs/call-mode-m1-checklist.md`. Additionally: after **Stop call mode** WhatsApp plays by itself again (the tap is gone), the default input is back, and **no engine stays on the GPU** (`ps -A | grep -c prxvc` shows the app only; Activity Monitor's GPU history falls). After `kill -9` and a new start, the default input is restored at start and WhatsApp is audible again as soon as the process is gone.

## Item 7: headphones and echo

1. With headphones: no warning, no "Paused while you listen".
2. Optional, only to see the behaviour: select the built-in speakers as the output, start translated. Expected: the warning "The sound goes out through the built-in speakers ..." and, while the translation plays, "Paused while you listen"; the other person's translation is not translated back. Go back to headphones.

## Item 8: the ducking level (CT-21, observe and write down)

The voice-processing unit of WhatsApp lowers other applications' sound by about 30 dB (measured outside a call). Our translated playback is another application's sound on the same device. Observe and record:

1. In the debug view, "Translated playback level: ... dBFS, peak ... dBFS" for the incoming lane while a translated clause plays (this is our playback before the device and before WhatsApp's ducking; it is measured after `call.output_gain_db`).
2. How loud the translation is to your ear compared with the same stand-in voice outside a call (play any audio from another application for reference), with `call.output_gain_db = 0`.
3. If it is much quieter during the call, set `call.output_gain_db` to +6, then +12 (the design bounds compensation at +12 dB) and Stop and Start again; note the value at which it sounds right. A peak near 0 dBFS at the higher gains means clipping: note it.
4. Whether the quiet level comes and goes with WhatsApp's own sound (the ducking depends on what WhatsApp plays).

Do not try to fix ducking; the result decides which of the design's options is worth building.

## Reading the debug view

Per lane: the languages; **captions** (committed clauses with their translation and a status, and the clause being spoken with the stable part in normal type and the unstable tail in grey italics); **counters** (late drops, backlog skips, stale skips, forced cuts, final rewrites, suspect translations, runaway synthesis, capture overruns, underruns, refine fallbacks); the **last clause's latency** (speech start to first translated audio, the committing pause to first translated audio, and every stage in ms: first partial, ASR final, MT first token and total, TTS first chunk and total, wait for playout); **what each party hears**; and the **issues**, each with what to do.

## Tracing a call into a latency report

Set `debug.call_trace = true` (and, if you accept the texts in a file on your disk, `debug.call_trace_text = true`) before Start. The file `traces/call-<time>.jsonl` in the data directory has one line per clause: the stage timestamps in nanoseconds, the intervals in milliseconds, the outcome, and with the text switch the recognised, translated and spoken texts. At most five files of 20 MB are kept. Send the file with your notes. Without the switches no file is written and no text leaves the screen.

## Stopping, and if something goes wrong

- Press **Stop call mode** (or quit from the tray menu): the default input is restored first, then the paths close, and WhatsApp plays by itself again.
- If the app dies: relaunch it; it restores the default input before anything else (you can also set it in System Settings, Sound, Input). While the client is dead and WhatsApp is silent, quit WhatsApp's call or relaunch the client: a tap that is gone gives WhatsApp's sound back.
- If you hear nothing from WhatsApp while the client runs: the issue list says whether the local output could not be opened (then the tap stays unmuted and WhatsApp plays itself) or whether the tap lost signal.
- Stop the server when you are done: `scripts/dev/llama-server.sh stop`.

## What to report

Per item: pass or fail, the debug view's state, lines and counters at a failure, and the end of the log (`~/Library/Application Support/org.openprx.prx-voice-client/logs/prx-voice.log`). The trace file of a call if you switched it on. For item 3 the sentences that came out wrong, as heard. For item 8 the numbers asked there.

## What the harness found (before the first call)

The silent harness (`call_harness`, see its header) runs one direction of the real pipeline on recorded speech without a call application, a microphone or any change of the default devices; its latency tables are in the report that came with this change. Two behaviours matter for your first call:

- A clause is committed at a pause of `pipeline.clause_pause_ms` (300) or at the end of the segment, so a sentence spoken without a pause is translated only after it ends.
- The backlog rule counts the length of the **source speech** of queued clauses. When a long clause is followed by a short one (for instance a trailing "um"), the sum can exceed `pipeline.max_backlog_ms` (6000) and the **older, long clause is skipped without being spoken**. If a long sentence is missing from the translation and the counters show a backlog skip, raise `pipeline.max_backlog_ms` (for example 20000) in `config.toml`.
