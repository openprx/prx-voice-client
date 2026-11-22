//! One INFO line per assistant turn and a summary at the end of the session: figures only, never the words said or the reply (the privacy settings decide what is kept, not the log).
//!
//! The host feeds it the events of the loop with the time it saw them; it measures from the phase
//! changes, so the figures are those of the host's view (a few tens of milliseconds coarse: the
//! host looks every 30 ms).

use std::time::{Duration, Instant};

use prxvc_session::assistant::{AssistantEvent, Phase};

/// What one turn measured.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Turn {
    speech: Duration,
    to_final: Option<Duration>,
    to_first_token: Option<Duration>,
    to_first_audio: Option<Duration>,
    reply_deltas: u32,
    interrupted: bool,
    faults: u32,
}

/// The log of one session.
#[derive(Default)]
pub(crate) struct TurnLog {
    speech_began: Option<Instant>,
    speech_ended: Option<Instant>,
    final_at: Option<Instant>,
    current: Turn,
    in_turn: bool,
    turns: Vec<Turn>,
    hums: u32,
    faults: u32,
    mic_closures: u32,
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn percentile(sorted: &[u64], p: usize) -> u64 {
    sorted
        .get((sorted.len() * p / 100).min(sorted.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0)
}

impl TurnLog {
    /// Take an event seen at `now`. Returns the line of a turn that just ended.
    pub(crate) fn on_event(&mut self, now: Instant, event: &AssistantEvent) -> Option<String> {
        match event {
            AssistantEvent::Phase(Phase::UserSpeaking) => {
                self.speech_began.get_or_insert(now);
            }
            AssistantEvent::Phase(Phase::Thinking) => {
                self.speech_ended.get_or_insert(now);
            }
            AssistantEvent::Phase(Phase::Speaking) => {
                if let (true, Some(at)) = (self.in_turn, self.final_at) {
                    self.current
                        .to_first_audio
                        .get_or_insert(now.saturating_duration_since(at));
                }
            }
            AssistantEvent::Phase(Phase::Listening) => {
                // Speech that never became a turn (a hum, filler): counted, no line.
                if !self.in_turn && self.speech_began.take().is_some() {
                    self.hums += 1;
                    self.speech_ended = None;
                }
            }
            AssistantEvent::UserTurn { .. } => {
                let ended = self.speech_ended.unwrap_or(now);
                self.current = Turn {
                    speech: self
                        .speech_began
                        .map_or(Duration::ZERO, |b| ended.saturating_duration_since(b)),
                    to_final: Some(now.saturating_duration_since(ended)),
                    ..Turn::default()
                };
                self.final_at = Some(now);
                self.in_turn = true;
            }
            AssistantEvent::ReplyDelta { .. } if self.in_turn => {
                if let Some(at) = self.final_at {
                    self.current
                        .to_first_token
                        .get_or_insert(now.saturating_duration_since(at));
                }
                self.current.reply_deltas += 1;
            }
            AssistantEvent::ReplyDone => return self.end_turn(false),
            AssistantEvent::ReplyCut => return self.end_turn(true),
            AssistantEvent::MicPaused(true) => self.mic_closures += 1,
            AssistantEvent::Fault(_) => {
                self.faults += 1;
                self.current.faults += 1;
            }
            _ => {}
        }
        None
    }

    fn end_turn(&mut self, interrupted: bool) -> Option<String> {
        if !self.in_turn {
            return None;
        }
        self.in_turn = false;
        self.speech_began = None;
        self.speech_ended = None;
        self.final_at = None;
        let mut turn = std::mem::take(&mut self.current);
        turn.interrupted = interrupted;
        let line = format!(
            "turn={} speech_ms={} end_of_speech_to_final_ms={} to_first_reply_token_ms={} to_first_reply_audio_ms={} reply_chunks={} interrupted={} faults={} filler_only_inputs_dropped={}",
            self.turns.len() + 1,
            ms(turn.speech),
            turn.to_final.map_or("-".into(), |d| ms(d).to_string()),
            turn.to_first_token
                .map_or("-".into(), |d| ms(d).to_string()),
            turn.to_first_audio
                .map_or("-".into(), |d| ms(d).to_string()),
            turn.reply_deltas,
            turn.interrupted,
            turn.faults,
            self.hums,
        );
        self.turns.push(turn);
        Some(line)
    }

    /// The summary line of the session.
    pub(crate) fn summary(&self) -> String {
        let sorted = |f: fn(&Turn) -> Option<Duration>| {
            let mut v: Vec<u64> = self.turns.iter().filter_map(f).map(ms).collect();
            v.sort_unstable();
            v
        };
        let fin = sorted(|t| t.to_final);
        let tok = sorted(|t| t.to_first_token);
        let aud = sorted(|t| t.to_first_audio);
        let pair = |v: &[u64]| format!("{}/{}", percentile(v, 50), percentile(v, 95));
        format!(
            "assistant session summary turns={} interruptions={} filler_only_inputs_dropped={} faults={} echo_gate_closures={} end_of_speech_to_final_ms(median/p95)={} to_first_reply_token_ms(median/p95)={} to_first_reply_audio_ms(median/p95)={}",
            self.turns.len(),
            self.turns.iter().filter(|t| t.interrupted).count(),
            self.hums,
            self.faults,
            self.mic_closures,
            pair(&fin),
            pair(&tok),
            pair(&aud),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn text(s: &str) -> String {
        s.to_owned()
    }

    #[test]
    fn a_turn_makes_one_line_of_figures_and_never_carries_the_words() {
        let t0 = Instant::now();
        let mut log = TurnLog::default();
        let events = [
            (0, AssistantEvent::Phase(Phase::UserSpeaking)),
            (1_500, AssistantEvent::Phase(Phase::Thinking)),
            (
                1_700,
                AssistantEvent::UserTurn {
                    text: text("secret words"),
                },
            ),
            (
                2_100,
                AssistantEvent::ReplyDelta {
                    text: text("a secret reply"),
                },
            ),
            (2_300, AssistantEvent::Phase(Phase::Speaking)),
            (2_400, AssistantEvent::ReplyDelta { text: text("more") }),
        ];
        for (when, e) in &events {
            assert!(log.on_event(at(t0, *when), e).is_none());
        }
        let line = log
            .on_event(at(t0, 5_000), &AssistantEvent::ReplyDone)
            .expect("a line");
        assert!(line.contains("speech_ms=1500"), "{line}");
        assert!(line.contains("end_of_speech_to_final_ms=200"), "{line}");
        assert!(line.contains("to_first_reply_token_ms=400"), "{line}");
        assert!(line.contains("to_first_reply_audio_ms=600"), "{line}");
        assert!(line.contains("reply_chunks=2 interrupted=false"), "{line}");
        assert!(
            !line.contains("secret"),
            "no spoken or reply text in the log"
        );
    }

    #[test]
    fn a_cut_reply_is_an_interruption_and_a_hum_is_counted_without_a_line() {
        let t0 = Instant::now();
        let mut log = TurnLog::default();
        log.on_event(at(t0, 0), &AssistantEvent::Phase(Phase::UserSpeaking));
        assert!(
            log.on_event(at(t0, 300), &AssistantEvent::Phase(Phase::Listening))
                .is_none()
        );
        log.on_event(at(t0, 1_000), &AssistantEvent::Phase(Phase::UserSpeaking));
        log.on_event(at(t0, 2_000), &AssistantEvent::Phase(Phase::Thinking));
        log.on_event(at(t0, 2_100), &AssistantEvent::UserTurn { text: text("x") });
        log.on_event(at(t0, 2_200), &AssistantEvent::MicPaused(true));
        let line = log
            .on_event(at(t0, 2_600), &AssistantEvent::ReplyCut)
            .expect("a line");
        assert!(line.contains("interrupted=true"), "{line}");
        let s = log.summary();
        assert!(
            s.contains("turns=1 interruptions=1 filler_only_inputs_dropped=1"),
            "{s}"
        );
        assert!(s.contains("echo_gate_closures=1"), "{s}");
    }

    #[test]
    fn a_session_without_turns_has_a_summary_of_zeros() {
        let s = TurnLog::default().summary();
        assert!(s.contains("turns=0 interruptions=0"), "{s}");
        assert!(s.contains("(median/p95)=0/0"), "{s}");
    }
}
