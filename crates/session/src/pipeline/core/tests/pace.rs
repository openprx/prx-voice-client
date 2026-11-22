//! Catching up with a queue of translated speech: the pace is announced before the audio it applies to, rises with the queue and falls back when it is empty.

use super::*;

fn paces(events: &[PipelineEvent]) -> Vec<(u32, bool)> {
    events
        .iter()
        .filter_map(|e| match e {
            PipelineEvent::Pace { speed, tight } => Some(((speed * 100.0).round() as u32, *tight)),
            _ => None,
        })
        .collect()
}

fn queue_of_four(h: &mut Harness) -> Vec<ClauseId> {
    h.speech(1);
    for i in 0..4u64 {
        h.commit(
            1,
            &format!("第{i}句话很长很长"),
            100 + i * 500,
            600 + i * 500,
        );
    }
    let ids: Vec<ClauseId> = (0..4u16).map(|i| cl(1, i)).collect();
    for c in &ids {
        h.translate(*c);
    }
    // The later clauses are synthesised while the first has not been released yet.
    for c in ids.iter().skip(1) {
        h.chunk(*c, 3_000);
        h.stage(*c, StageEvent::TtsDone { clause: *c });
    }
    ids
}

fn play(h: &mut Harness, c: ClauseId) {
    let now = MonoTime::from_millis(1);
    h.command(Command::PlaybackStarted { clause: c, at: now });
    h.command(Command::PlaybackFinished { clause: c, at: now });
}

#[test]
fn a_queue_speeds_the_next_clauses_up_before_their_audio_and_the_end_of_it_returns_to_normal() {
    let mut h = harness(|_| {});
    let ids = queue_of_four(&mut h);
    h.chunk(ids[0], 3_000);
    h.stage(ids[0], StageEvent::TtsDone { clause: ids[0] });
    let first = h.drain();
    assert_eq!(
        paces(&first),
        [(100, true)],
        "the first clause is still being synthesised when it is released: normal speed, short pauses"
    );
    let pace_at = first
        .iter()
        .position(|e| matches!(e, PipelineEvent::Pace { .. }))
        .unwrap();
    let audio_at = first
        .iter()
        .position(|e| matches!(e, PipelineEvent::Audio(_)))
        .unwrap();
    assert!(
        pace_at < audio_at,
        "announced before the audio it applies to"
    );

    let mut later = Vec::new();
    for c in &ids[..3] {
        play(&mut h, *c);
        later.extend(h.drain());
    }
    play(&mut h, ids[3]);
    later.extend(h.drain());
    let seen = paces(&later);
    assert!(
        seen.contains(&(125, true)),
        "a complete clause with 6 s behind it is sped up: {seen:?}"
    );
    assert_eq!(
        seen.last(),
        Some(&(100, false)),
        "back to normal at the end: {seen:?}"
    );
    assert!(seen.iter().all(|(s, _)| (100..=125).contains(s)));
}

#[test]
fn switched_off_nothing_speeds_up() {
    let mut h = harness(|p| p.catch_up.enabled = false);
    let ids = queue_of_four(&mut h);
    h.chunk(ids[0], 3_000);
    h.stage(ids[0], StageEvent::TtsDone { clause: ids[0] });
    let mut seen = paces(&h.drain());
    play(&mut h, ids[0]);
    seen.extend(paces(&h.drain()));
    assert!(seen.iter().all(|(s, _)| *s == 100) && seen.iter().any(|(_, t)| *t));
}

#[test]
fn a_single_clause_is_never_paced() {
    let mut h = harness(|_| {});
    h.speech(1);
    h.commit(1, "第一句话很长", 100, 600);
    let c = cl(1, 0);
    h.translate(c);
    h.chunk(c, 2_000);
    h.stage(c, StageEvent::TtsDone { clause: c });
    assert!(paces(&h.drain()).is_empty());
}
