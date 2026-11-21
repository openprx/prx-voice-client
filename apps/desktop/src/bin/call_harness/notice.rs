//! The spoken notice in a harness run: synthesised once, then queued on the translated lane either when the speech begins (the product's behaviour) or just before the first translated audio (the earlier one), so that the effect on the first utterance's delay can be measured.

use std::sync::Arc;

use prxvc_media::{ClauseId, Direction, Lang, PcmChunk, SegmentId};
use prxvc_ports::{CancellationToken, SpeechSynth, TtsItem, TtsRequest, VoiceSelection};

/// When the notice is queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NoticeMode {
    /// No notice.
    Off,
    /// As the speech begins.
    Speech,
    /// Just before the first translated audio.
    Audio,
}

impl NoticeMode {
    pub(crate) fn parse(s: &str) -> Result<Self, String> {
        match s {
            "off" => Ok(Self::Off),
            "speech" => Ok(Self::Speech),
            "audio" => Ok(Self::Audio),
            other => Err(format!("unknown notice mode {other} (off, speech or audio)")),
        }
    }
}

/// The notice, ready to queue.
#[derive(Clone)]
pub(crate) struct Notice {
    pub mode: NoticeMode,
    pub chunks: Arc<Vec<PcmChunk>>,
}

/// The clause the notice uses; no real segment reaches this number.
pub(crate) fn notice_clause(direction: Direction) -> ClauseId {
    ClauseId::new(
        SegmentId::new(direction, u64::MAX).expect("seq >= 1"),
        0,
    )
}

/// Whether `clause` is the notice's.
pub(crate) fn is_notice(clause: ClauseId) -> bool {
    clause.segment().seq() == u64::MAX
}

/// Synthesise `text` into memory with the direction's own synthesiser and voice.
pub(crate) fn synthesise(
    tts: &dyn SpeechSynth,
    direction: Direction,
    text: &str,
    lang: Lang,
    voice: VoiceSelection,
) -> Result<(Vec<PcmChunk>, f64), String> {
    let mut stream = tts
        .synthesize(TtsRequest {
            clause: notice_clause(direction),
            text: text.to_owned(),
            lang,
            voice,
        })
        .map_err(|e| e.to_string())?;
    let cancel = CancellationToken::new();
    let mut chunks = Vec::new();
    loop {
        match stream.next(&cancel) {
            Some(Ok(TtsItem::Chunk(c))) => chunks.push(c),
            Some(Ok(TtsItem::Done { .. })) => break,
            Some(Err(e)) => return Err(e.to_string()),
            None => return Err("synthesis ended early".to_owned()),
        }
    }
    let seconds = chunks.iter().map(|c| c.duration().as_secs_f64()).sum();
    Ok((chunks, seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mode_names_parse() {
        assert_eq!(NoticeMode::parse("speech"), Ok(NoticeMode::Speech));
        assert_eq!(NoticeMode::parse("audio"), Ok(NoticeMode::Audio));
        assert_eq!(NoticeMode::parse("off"), Ok(NoticeMode::Off));
        assert!(NoticeMode::parse("never").is_err());
    }

    #[test]
    fn the_notice_clause_is_recognised_and_a_real_one_is_not() {
        let n = notice_clause(Direction::Outgoing);
        assert!(is_notice(n));
        assert!(!is_notice(ClauseId::new(SegmentId::first(Direction::Outgoing), 0)));
    }
}
