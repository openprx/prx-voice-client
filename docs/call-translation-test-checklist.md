# Translated call: first real test (milestones M2 and M3)

Status: for the owner; written with the work package that composes both directions (CT-15, CT-16 and the minimum of CT-17).
Source: pipeline-design sections 4.5, 4.6, 5.4, 6 and 9.4 (items 3, 4 and 8), and `docs/call-mode-m1-checklist.md` for the audio-only items that still apply.

What this test shows: you press **Start** with **Translate** on, make a WhatsApp call, and both directions are translated in the client. The other person hears your speech translated and spoken in your cloned voice. You hear the other person translated, spoken in a stand-in voice. Nothing here was run in a real call before; everything below that needs a device, WhatsApp or a permission prompt is yours.

## What the build does

- **Outgoing** (you speak Mandarin, the other person hears English): microphone, voice detection, recognition (a streaming draft, then a second decode of each clause), translation by the selected engine (system translation by default, or the local language model), synthesis in your cloned voice (embedding-only mode), the mixer, the virtual microphone. What the other person hears is decided by the direction's state machine and announced in the Call translation window: the translation, your own voice (while paused or after a failure, the default), or nothing.
- **Incoming** (the other person speaks English, you hear Chinese): the client taps WhatsApp **muted while tapped**, so you do not hear the original twice and WhatsApp comes back by itself if the client dies. The client plays to your output through its own mixer: first the original (so you are never left in silence), then the translation per `call.local_output_mode` (default `translation`: translation only; `both`: the original lowered by `call.original_gain_db`, -12 dB, under the translation; `original`: the original only). The voice is the preset named in `voice.incoming_preset`.
- **Audio only** (the Translate switch off) is unchanged: unmuted tap, nothing played by the client.
- **Headphones are required.** On the built-in speakers the client shows a warning (the output is the built-in speaker) and, with `audio.echo_mode = auto`, closes the microphone while it plays ("Paused while you listen", PD-6.3).
- **Translation engine** (`call.translation_engine`, Settings, Call translation): **system translation** is the default on macOS 26 and later (Apple's on-device Translation framework: fast, light, translates each sentence on its own, no context); the **language model** (Gemma 4 E4B on a local `llama-server`) is the alternative. **No server needs to be started by hand any more**: selecting the language model makes the application start the local server (status in the settings: starting, ready, failed with the reason); selecting system translation stops it; the assistant starts it on demand and it stops again 60 s after the last use (`llm_server.idle_stop_s`). A server you started yourself on port 8080 is used as it is and never stopped by the application.
- **The Skip command exists in the coordinator but has no button yet**; Pause and Resume pause both directions.
- **Filler is not translated.** A recognised unit that is only a hum or hesitation (嗯, 呃, 啊, 哦, 唔, um, uh, mm, hmm, mhm, and punctuation only) is dropped before translation: it is counted ("Dropped filler" in the window, `filler_dropped` in the counters) and never spoken. Real answers are kept (好, 对, 是, yes, no, ok, 嗯嗯好的). A one-word "嗯" or "Mm." meant as an acknowledgement is dropped too, by design; `pipeline.drop_filler = false` turns the rule off.

## Before you start (development set-up)

1. Headphones on, and selected as the macOS output (System Settings, Sound, Output). Not the built-in speakers.
2. Once, or after changing models or voices (all safe to repeat; details in `scripts/dev/README.md`):

   ```bash
   scripts/dev/adopt-models.sh      # models into the store, checked against the catalogue pins
   scripts/dev/install-voices.sh    # your voice (owner-zh, owner-en), the stand-in preset, config keys
   ```

   The preset `dev-standin` is a **stand-in made with the system synthesizer**, not a product voice and not your voice.
3. Translation engine. With the default (system translation) nothing has to be started; the Chinese and English language packs must be installed (System Settings, General, Language & Region, Translation Languages: download Chinese (Simplified) and English; `scripts/dev/preflight.sh` and the settings page say when one is missing). For the language-model engine choose it in the settings: the application starts the server itself. `scripts/dev/llama-server.sh start` still works for a server of your own, which the application then adopts.

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
   | `pipeline.clause_pause_ms`, `pipeline.early_commit` | when a clause is committed: `early_commit` is `segment_end` (only at a pause or the end of the segment), `punctuation` (also from the recogniser's stable text at sentence-final and clause-level marks) or `chunk` (default: also, with no mark, once the stable text has `pipeline.early_chunk_words` English words or `pipeline.early_chunk_chars` Chinese characters, 6 and 8; after `pipeline.early_max_wait_ms` (2500) without a commit the chunk size is halved). See "What the harness found" |
   | `pipeline.drop_filler` (default `true`) | drop recognised units that are only filler (see "What the build does"). Set `false` to translate and speak every unit |
   | `pipeline.skip_source_in_target` (default `true`) | a recognised clause that is already in the language the listener hears (the other party spoke your language, or a Chinese sentence reached the Chinese listener) is not translated or synthesised; the original voice passes through for that speech and the caption says "Not translated: already in <language>". Judged from the script of the recognised text (the recogniser reports no language); text of fewer than 4 letters is never judged |
   | `pipeline.retry_wrong_language` (default `true`) | a translation that starts in the wrong script is asked for once more with a stricter instruction before anything of it is spoken; a second miss fails the clause visibly and the direction's `*_on_failure` applies. The other assessment flags (too long, cut off, refusal, answer-like, thinking text) are counted per kind and shown on the caption; the clause is spoken regardless |
   | `call.disclosure` (default `true`), `call.disclosure_text_en`, `call.disclosure_text_zh` | the spoken notice: before the first translated sentence of the outgoing direction in each call mode session, the other party hears once, in their language, that the call is translated in real time by a synthetic voice. Empty texts use the built-in wording; **Say it again** in the window plays it again. Not translated, not filler, not skipped by the backlog or barge-in rules |
   | `call.captions_size` (`small`, `medium`, `large`) | the caption text size of the Call translation window |
   | `pipeline.min_speech_dbfs` (default `-75`) | a start of speech whose first frames average below this level (dBFS) is not a segment and is never decoded; `-100` turns it off. Measured: benchmark speech averages down to -65 dBFS and the voiced hums that become "嗯" are at -19 to -28 dBFS, so a level cannot separate them; the floor only removes sounds at the level of dither |
   | `pipeline.backlog_skipping`, `pipeline.max_backlog_ms`, `pipeline.max_lag_ms` | keeping up with a speaker who talks continuously: a queued clause is skipped only when the audio queued for you is above `max_backlog_ms` (15000) **and** that clause's speech ended more than `max_lag_ms` (20000) ago. `backlog_skipping = false` never skips: everything is played, however late. See "What the harness found" |

6. Open the app and the **Call translation** window (Chinese: **通话翻译**): `open -n "dist/debug/PRX Voice.app"`, then choose **Call translation** at the top of the menu-bar menu or in the orb's right-click menu. The window holds the state line, the Start/Stop button, Pause/Resume, the Translate switch, the two languages, the notes (issues with their buttons) and the live captions of both directions; it is the one place the steps below use. (The Debug window keeps only the raw counters and a link to it.) If the assistant session holds the microphone, the window says so and offers **End assistant and start**, which ends the session and starts call mode with the same options once the microphone is free; clicking the orb a second time while the assistant listens also ends the session. Start WhatsApp and leave it idle.

The first Start asks for **Microphone** and **System Audio Recording**; allow both.

## Item 1: start translated

1. Turn **Translate** on, press **Start**. Loading takes several seconds (models, then the voice): the state shows "Audio ready (passthrough)" first, so the other person hears your own voice meanwhile, then "Translating".
2. Expected: both lanes show "translating" with their languages (zh to en outgoing, en to zh incoming), "The other person hears: the translation", "You hear: the translation", no red issue.
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

1. With the language-model engine, stop the model server in the middle of a call (`kill` the `llama-server` process the application started, or `scripts/dev/llama-server.sh stop` for your own), then speak. Expected: after a failed clause the other person hears your original voice, the lane says "the other person hears: your own voice", a failure shows in the issues, and the translation returns by itself: the application restarts a server it started (up to three times in five minutes, with a growing delay). With system translation there is no server to stop; to see its problem path, remove a language pack in System Settings and start a call: the issue says the pack is missing and offers "Open translation languages" and "Switch to the language model".
2. Switch the default input away and back as in item 5 of the M1 checklist.

## Item 6: WhatsApp quits, sleep, stop, crash

As items 6 and 7 of `docs/call-mode-m1-checklist.md`. Additionally: after **Stop call mode** WhatsApp plays by itself again (the tap is gone), the default input is back, and **no engine stays on the GPU** (`ps -A | grep -c prxvc` shows the app only; Activity Monitor's GPU history falls). After `kill -9` and a new start, the default input is restored at start and WhatsApp is audible again as soon as the process is gone.

## Item 7: headphones and echo

1. With headphones: no warning, no "Paused while you listen".
2. Optional, only to see the behaviour: select the built-in speakers as the output, start translated. Expected: the warning "The sound goes out through the built-in speakers ..." and, while the translation plays, "Paused while you listen"; the other person's translation is not translated back. Go back to headphones.

## Item 8: the ducking level (CT-21, observe and write down)

The voice-processing unit of WhatsApp lowers other applications' sound by about 30 dB (measured outside a call). Our translated playback is another application's sound on the same device. Observe and record:

1. In the Debug window's call section, "Translated playback level: ... dBFS, peak ... dBFS" for the incoming lane while a translated clause plays (this is our playback before the device and before WhatsApp's ducking; it is measured after `call.output_gain_db`).
2. How loud the translation is to your ear compared with the same stand-in voice outside a call (play any audio from another application for reference), with `call.output_gain_db = 0`.
3. If it is much quieter during the call, set `call.output_gain_db` to +6, then +12 (the design bounds compensation at +12 dB) and Stop and Start again; note the value at which it sounds right. A peak near 0 dBFS at the higher gains means clipping: note it.
4. Whether the quiet level comes and goes with WhatsApp's own sound (the ducking depends on what WhatsApp plays).

Do not try to fix ducking; the result decides which of the design's options is worth building.

## Reading the Call translation window and the debug counters

The window shows per lane: what its listener hears, the level, the **captions** (committed clauses with their translation and a status, and the clause being spoken with the stable part in normal type and the unstable tail in grey italics), a note with the dropped filler and the skipped clauses, and the last clause's latency. The Debug window's call section keeps the raw numbers. Per lane there: the languages; **captions** (committed clauses with their translation and a status, and the clause being spoken with the stable part in normal type and the unstable tail in grey italics); **counters** (late drops, backlog skips, stale skips, forced cuts, final rewrites, suspect translations, runaway synthesis, capture overruns, underruns, refine fallbacks, dropped filler, unspeakable pieces dropped); the **last clause's latency** (speech start to first translated audio, the committing pause to first translated audio, and every stage in ms: first partial, ASR final, MT first token and total, TTS first chunk and total, wait for playout); **what each party hears**; and the **issues**, each with what to do.

## Tracing a call into a latency report

Set `debug.call_trace = true` (and, if you accept the texts in a file on your disk, `debug.call_trace_text = true`) before Start. The file `traces/call-<time>.jsonl` in the data directory has one line per clause: the stage timestamps in nanoseconds, the intervals in milliseconds, the outcome, and with the text switch the recognised, translated and spoken texts. At most five files of 20 MB are kept. Send the file with your notes. Without the switches no file is written and no text leaves the screen.

## Stopping, and if something goes wrong

- Press **Stop** (or quit from the tray menu): the default input is restored first, then the paths close, and WhatsApp plays by itself again.
- If the app dies: relaunch it; it restores the default input before anything else (you can also set it in System Settings, Sound, Input). While the client is dead and WhatsApp is silent, quit WhatsApp's call or relaunch the client: a tap that is gone gives WhatsApp's sound back.
- If you hear nothing from WhatsApp while the client runs: the issue list says whether the local output could not be opened (then the tap stays unmuted and WhatsApp plays itself) or whether the tap lost signal.
- The application stops the model server it started when it quits (and after a crash, the next start stops the one it left behind). A server you started yourself is yours to stop.

## What to report

Per item: pass or fail, the debug view's state, lines and counters at a failure, and the end of the log (`~/Library/Application Support/org.openprx.prx-voice-client/logs/prx-voice.log`). The trace file of a call if you switched it on. For item 3 the sentences that came out wrong, as heard. For item 8 the numbers asked there.

## What the harness found (before the first call)

The silent harness (`call_harness`, see its header) runs one direction of the real pipeline on recorded speech without a call application, a microphone or any change of the default devices. The full tables, the conditions of the runs and every committed unit with its translation are in `docs/call-translation-early-commit-sample.md`. In short:

- **First translated audio.** Measured from the moment the speaker starts, with the earlier measurement set (15 utterances per language, medians): Mandarin source 5.2 s at the pause or segment end (`segment_end`), 4.3 s with `punctuation`, 3.5 s with `chunk`; English source 4.8 s, 4.8 s and 3.2 s. The target of 1.2 to 2.5 s is **not** reached: the floor is the recogniser's stability rule (a word is declared stable after four agreeing passes 400 ms apart, and the last six words of a growing text are never declared stable), which puts the first stable text about 2.5 to 3 s after the speaker starts. Lowering `pipeline.asr_stable_agreement` and `pipeline.asr_stable_tail_guard` is the lever for that, at the cost of recognition accuracy; it was not changed. For monologues of 45 to 53 s the first audio comes after about 3 s instead of 7 to 8 s.
- **Commit behaviours** (`pipeline.early_commit`): `segment_end` commits only at a pause of `pipeline.clause_pause_ms` (300) or at the end of the segment; `punctuation` also commits the recogniser's stable text up to its last sentence-final or clause-level mark (never inside a number, time or abbreviation, never between a date and its year); `chunk` (the default) also commits, when there is no mark, once the stable text has `pipeline.early_chunk_words` (6) English words or `pipeline.early_chunk_chars` (8) Chinese characters, at the last boundary that does not split a number with its unit, a time, a date, a name, a common Chinese word, or end on a function word, and that leaves at least two units after it; after `pipeline.early_max_wait_ms` (2500) without a commit the chunk size is halved. Each unit is translated with the earlier units of the same utterance as context (`pipeline.mt_context_clauses`, 4).
- **Translation quality with `chunk`.** English fragments translate well; the damage is small (an occasional literal fragment such as "of the almost every aspect" repeating a word of the previous unit). Mandarin fragments are noticeably worse than whole sentences: the meaning can shift ("下午三点十五分开会" alone became "The meeting will be adjourned at 3:15 PM"), a unit can open with a phrase that was already translated, and the cuts fall in the middle of a sentence ("我下周三下午三点不能参加 | 会议，请改到周四上午十点" gives "I can't make it ... | Meeting, please change it ..."). Numbers, dates and names stayed intact in every sample (both languages), so `chunk` is the default for the speed it buys; if Mandarin output sounds choppy or wrong, set `pipeline.early_commit = "punctuation"` (smaller gain, cuts only at marks) or `"segment_end"` (the earlier behaviour).
- **Early commit and the refine pass.** With the planned two-pass recognition the clause audio is re-decoded by the refine model before translation. A cut chosen from stable text has no known audio position, so units committed early, and what follows them until the next pause, are translated from the recogniser's stable text, not the refined text. Clauses after a pause are refined as before. Finals that disagree with committed text are counted (`final_rewrites`); in the measured sets 0 to 2 items of 15 had one, and the committed text was never changed.
- **Keeping up with the speaker.** The backlog rule no longer counts the length of the source speech. It measures what the listener is behind: the real synthesised audio of queued clauses (for clauses not yet synthesised, the speaking time of their translation), and how long ago each clause's speech ended. A clause is skipped only if both the queued audio is above `pipeline.max_backlog_ms` (15000) and the clause is older than `pipeline.max_lag_ms` (20000); the one playing is never cut, order is never changed and the newest clause is kept. Every skip is counted (`backlog_skips`), traced, shown in the direction snapshot (`recent_skips` with the source text, so the captions can show what was not spoken) and puts the direction in "Skipping to keep up". With the default thresholds the harness measured no backlog skip in any run (the earlier rule skipped 2 of 15 Mandarin and 3 to 4 of 15 English items). Trade-off: with `pipeline.backlog_skipping = false` nothing is ever lost to the backlog, but after a long stretch of continuous speech you can hear the translation tens of seconds late; with it on, you stay near the speaker and lose the oldest stale clauses, visibly.
- **A different loss that remains: barge-in.** With `call.barge_in = true` (the default, decision D-P5) a clause still waiting when the same speaker starts a new segment more than `call.barge_in_max_lag_ms` (3000) after its speech ended is skipped as stale. In the 45 to 53 s monologues this skipped 2 to 6 clauses per item in every commit behaviour (none in the ordinary set with the default `chunk`, and none with `call.barge_in = false`). It is counted (`stale_skips`) and shown like the backlog skips, but it is not the backlog rule and was not changed. With `call.barge_in = false` the last audio of a 50 s monologue finished 2 to 4 s later.
- **Saved settings keep the old values.** The settings file keeps every `[pipeline]` key it was saved with: an install that saved `max_backlog_ms = 6000` or `mt_context_clauses = 2` keeps them, and the new rule then needs the queued audio above 6 s **and** a clause older than `max_lag_ms`. The new keys (`early_commit` and the others) are missing in such a file and take their defaults. `call_harness --default-pipeline` ignores the saved `[pipeline]` section.

## What the first real call found (2026-11-21) and what changed

The first translated WhatsApp call produced many spoken "嗯" and "Mm.". The log of that call (07:00:04Z to 07:05:07Z) shows the built-in speakers (the echo gate was active), `Degraded { Outgoing: RefineFailing }` at 07:01:09, two `BacklogSkipping` periods and three `synthesis ran away` warnings (`out#6.2`, `out#6.3`, `out#7.0`, 3.28 s each, `attempt=0`). It was reproduced without a call with `call_harness` (inputs made by `bench/asr/scripts/make_noise_inputs.py`; `call_harness --own-set <group> --set zh|en`, tag `before`/`after`).

- **Cause of the "嗯".** The recogniser (Qwen3-ASR) writes "嗯。" for audio without speech, and the pipeline translated and spoke every unit it committed. The sources, measured: (1) a voiced hum or breath of the speaker that the VAD calls speech (6 of 6 synthetic hums, at -19 to -28 dBFS and 128 to 448 ms of voiced audio, became "嗯。" or "Hmm."/"Um."/"Oh." and were spoken as "Mm.", "Mhm.", 嗯); (2) a **trailing fragment**: after real speech, the final decode of the segment appended "嗯。", "。" or a lone word ("The.", "You.", "Right.", "Yes.") as one more unit. This happened even on clean read speech: before the fix **10 of the 15 Mandarin utterances and 1 of 15 English ones** of the ordinary set ended with a spoken filler unit, and 5 of 5 speech-then-noise inputs (noise at -40, -30 and -20 dBFS, a breath, a hum) did in Mandarin and 2 of 5 in English. Synthetic room noise (-60 to -20 dBFS), breaths and tap/bump bursts never passed the VAD (0 segments, so no decode). On built-in speakers the translated sound that reaches the microphone adds more of the same.
- **Fix 1: the filler filter** (`pipeline.drop_filler`, default `true`; `crates/session/src/filler.rs`). A committed unit whose tokens are all in the language's filler table (or the table of hums shared by both languages) is not translated or spoken; it is counted (`filler_dropped`, "Dropped filler" in the window) and traced (`filler_dropped`). The tables: Chinese 嗯 呃 额 唔 啊 哦 噢 喔 呣; English um uh er erm mm hm hmm mhm ah eh oh and the lone function words the recogniser leaves for faint sounds (the, a, an, and, of, to); punctuation-only text is filler too. Real answers are kept: 好, 对, 是, yes, no, ok, okay, 嗯嗯好的, "Uh, okay", "Oh no". A genuine one-word "嗯" or "Mm." acknowledgement is dropped as well, by design. A refine pass that hears only a hum where the streaming text had words does not replace the words.
- **Fix 2: the start gate** (`pipeline.min_speech_ms` 160, unchanged; new `pipeline.min_speech_dbfs`, default -75). Measured with `call_harness --vad-profile`: real short words are 320 to 576 ms of voiced audio, the hums 128 to 448 ms, so no duration separates them; benchmark speech averages down to **-65 dBFS** (the first attempt at -55 dBFS lost two real English utterances and was replaced), the hums are at -19 to -28 dBFS, so no level separates them either. The floor (10 dB below the quietest speech measured) only removes sounds at the level of dither. The filler filter, not the VAD, is what stops the "嗯".
- **Fix 3: no recogniser confidence is available.** transcribe.cpp exposes no-speech probability or token confidence only for its Whisper architecture; the Qwen3-ASR segment carries times and text only. The adapter keeps its digital-silence rule (peak below 1e-4); everything else is text matching.
- **Fix 4: nothing unspeakable reaches the synthesiser** (`prxvc_media::has_speakable_content`): a translated piece with no letter, digit or CJK character is never sent (`unspeakable_dropped`); a whole translation like that is a visible suspect-translation failure of the clause, as an empty one was; the Qwen3-TTS adapter rejects such text with a 400 as well.
- **`RefineFailing`.** Raised when the refine pass failed three times in a row (`pipeline.refine_failures_to_degrade`, 3). The pass "failed" not only on errors but whenever it returned **no text**, which is what it returns for the clip of a trailing fragment or a filler (the clip is its pad of silence). Those fragments are what the call produced, so three in a row degraded the direction (both sides hear the original) for a reason that is not a failure. An empty refine result now keeps the streaming text and no longer counts; real errors still count. What the log cannot tell: whether a real refine error also happened in that call (the text was off).
- **The runaway warnings.** `attempt=0` means no retry was configured: the adapter default was `RunawayRetry::Off` and nothing in the call set it, so nothing was retried; and with `Off` the audio of a clause streams while it is produced, so the 3.28 s of a runaway at the minimum cap (3.2 s, the clauses of one second of speech such as "Mm.") reached the playout before the guard fired. The listener could hear up to about 3 s of garbage. Now the live call uses `RunawayRetry::ShortOnce`: a clause at the minimum cap is held until it ends inside its cap (a fraction of a second at the measured real-time factor 0.2) and retried once with another seed, so a runaway is never played; longer clauses keep streaming without a retry (a runaway there is still heard until the guard fires; left as it is).
- **Before and after** (silent harness, the 15-utterance sets, the saved pipeline settings; medians): Mandarin 58 units before, 49 after, first translated audio 3.07 s to 3.09 s, last translated audio finished 5.94 s to 5.54 s after the speech ended; English 74 units before, 73 after, 3.12 s to 3.07 s and 6.96 s to 7.04 s; no word of content lost (the differences are recogniser variance between runs, for example "收缩象" before and "收缩现象" after). Noise inputs (48 runs before; room, breath and burst inputs need no change as they never open a segment): 20 filler units spoken before, 0 spoken and 20 counted as dropped after.
