//! Both call directions run the stage ports against one shared translator and one shared synthesiser.
//!
//! This is the executable form of the sharing rules in the pipeline design:
//! the engines are shared, every request has its own stream, and cancelling
//! one direction's request never touches the other's. It uses the ports only
//! through trait objects, the way a pipeline in another crate will.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use prxvc_media::{ClauseId, Direction, Lang, ManualClock, MonoTime, SegmentId};
use prxvc_ports::fakes::{
    AsrScript, EventKind, FakeSpeechSynth, FakeStreamingAsr, FakeTranslator, MtScript, TtsScript,
};
use prxvc_ports::{
    AsrContext, CancellationToken, MtDelta, MtRequest, SpeechSynth, StreamingAsr, Translator,
    TtsItem, TtsRequest, VoiceSelection,
};

const MS: fn(u64) -> Duration = Duration::from_millis;

/// What one direction produced for one clause.
struct Outcome {
    recognised: String,
    translated: String,
    audio_chunks: usize,
    cancelled: bool,
}

/// Recognise, translate and synthesise one clause of `direction`.
fn run_clause(
    direction: Direction,
    asr: &dyn StreamingAsr,
    mt: &dyn Translator,
    tts: &dyn SpeechSynth,
    cancel: &CancellationToken,
) -> Outcome {
    let clause = ClauseId::new(SegmentId::first(direction), 0);
    let (source, target) = match direction {
        Direction::Outgoing => (Lang::Zh, Lang::En),
        Direction::Incoming => (Lang::En, Lang::Zh),
    };

    let mut stream = asr.open(source, AsrContext::default()).expect("open asr");
    stream
        .feed(&[0.0; 320], MonoTime::from_millis(20))
        .expect("feed");
    let recognised = stream.finalize(cancel).expect("final").text;

    let request = MtRequest {
        clause,
        source,
        target,
        text: recognised.clone(),
        context: Vec::new(),
        glossary: Vec::new(),
    };
    let mut translation = mt.translate(request).expect("translate");
    let mut translated = String::new();
    while let Some(delta) = translation.next(cancel) {
        match delta.expect("mt delta") {
            MtDelta::Text(t) => translated.push_str(&t),
            MtDelta::Done { .. } => break,
        }
    }

    let voice = VoiceSelection::Preset {
        name: "preset-a".to_owned(),
    };
    let mut speech = tts
        .synthesize(TtsRequest {
            clause,
            text: translated.clone(),
            lang: target,
            voice,
        })
        .expect("synthesize");
    let (mut audio_chunks, mut cancelled) = (0, false);
    while let Some(item) = speech.next(cancel) {
        match item {
            Ok(TtsItem::Chunk(_)) => audio_chunks += 1,
            Ok(TtsItem::Done { .. }) => break,
            Err(e) => {
                cancelled = e.is_cancelled();
                break;
            }
        }
    }
    Outcome {
        recognised,
        translated,
        audio_chunks,
        cancelled,
    }
}

struct Providers {
    asr: Arc<dyn StreamingAsr>,
    mt: Arc<dyn Translator>,
    tts: Arc<dyn SpeechSynth>,
}

fn providers(clock: &ManualClock) -> (Providers, FakeSpeechSynth) {
    let asr = FakeStreamingAsr::new(clock.clone(), AsrScript::new([], "unused"))
        .with_script_for(Lang::Zh, AsrScript::new(["ni"], "ni hao"))
        .with_script_for(Lang::En, AsrScript::new(["he"], "hello"));
    let mt = FakeTranslator::new(clock.clone())
        .with_script("ni hao", MtScript::tokens(["Hel", "lo"]))
        .with_script("hello", MtScript::tokens(["ni ", "hao"]));
    let tts = FakeSpeechSynth::new(clock.clone())
        .with_default_script(TtsScript::chunks(3, 480).with_chunk_delay(MS(20)));
    let providers = Providers {
        asr: Arc::new(asr),
        mt: Arc::new(mt),
        tts: Arc::new(tts.clone()),
    };
    (providers, tts)
}

#[test]
fn both_directions_complete_on_shared_engines() {
    let clock = ManualClock::new();
    let (p, tts) = providers(&clock);

    let run = |direction| {
        let (asr, mt, tts) = (Arc::clone(&p.asr), Arc::clone(&p.mt), Arc::clone(&p.tts));
        thread::spawn(move || {
            run_clause(
                direction,
                asr.as_ref(),
                mt.as_ref(),
                tts.as_ref(),
                &CancellationToken::new(),
            )
        })
    };
    let (outgoing, incoming) = (run(Direction::Outgoing), run(Direction::Incoming));

    // Each worker blocks on the first synthesised chunk until the clock moves.
    tts.log().wait_for(EventKind::Opened, 2);
    for _ in 0..4 {
        clock.advance(MS(20));
    }
    let (outgoing, incoming) = (
        outgoing.join().expect("outgoing"),
        incoming.join().expect("incoming"),
    );

    assert_eq!(
        (outgoing.recognised.as_str(), outgoing.translated.as_str()),
        ("ni hao", "Hello")
    );
    assert_eq!(
        (incoming.recognised.as_str(), incoming.translated.as_str()),
        ("hello", "ni hao")
    );
    assert_eq!((outgoing.audio_chunks, incoming.audio_chunks), (3, 3));
    assert!(!outgoing.cancelled && !incoming.cancelled);
}

#[test]
fn skipping_one_direction_cancels_its_synthesis_and_leaves_the_other_playing() {
    let clock = ManualClock::new();
    let (p, tts) = providers(&clock);
    let (cancel_out, cancel_in) = (CancellationToken::new(), CancellationToken::new());

    let run = |direction, token: &CancellationToken| {
        let (asr, mt, tts, token) = (
            Arc::clone(&p.asr),
            Arc::clone(&p.mt),
            Arc::clone(&p.tts),
            token.clone(),
        );
        thread::spawn(move || {
            run_clause(direction, asr.as_ref(), mt.as_ref(), tts.as_ref(), &token)
        })
    };
    let (outgoing, incoming) = (
        run(Direction::Outgoing, &cancel_out),
        run(Direction::Incoming, &cancel_in),
    );

    tts.log().wait_for(EventKind::Opened, 2);
    clock.advance(MS(20)); // the first chunk of each direction is due
    tts.log().wait_for(EventKind::Item, 2);
    cancel_out.cancel(); // the user skips the outgoing lane
    let outgoing = outgoing.join().expect("outgoing");
    assert!(outgoing.cancelled);
    assert_eq!(
        outgoing.audio_chunks, 1,
        "nothing more is produced after the skip"
    );

    clock.advance(MS(60));
    let incoming = incoming.join().expect("incoming");
    assert!(!incoming.cancelled);
    assert_eq!(
        incoming.audio_chunks, 3,
        "the other direction plays its whole clause"
    );
    assert_eq!(
        tts.log().kinds_for("out#1.0").last(),
        Some(&EventKind::Cancelled)
    );
    assert_eq!(
        tts.log().kinds_for("in#1.0").last(),
        Some(&EventKind::Completed)
    );
}

#[test]
fn every_port_object_can_move_between_threads() {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    fn assert_send<T: Send + ?Sized>() {}
    assert_send_sync::<dyn prxvc_ports::VadProvider>();
    assert_send_sync::<dyn StreamingAsr>();
    assert_send_sync::<dyn prxvc_ports::RefineAsr>();
    assert_send_sync::<dyn Translator>();
    assert_send_sync::<dyn SpeechSynth>();
    assert_send_sync::<dyn prxvc_ports::TraceSink>();
    assert_send::<dyn prxvc_ports::VadEngine>();
    assert_send::<dyn prxvc_ports::AsrStream>();
    assert_send::<dyn prxvc_ports::MtStream>();
    assert_send::<dyn prxvc_ports::TtsStream>();
    assert_send::<dyn prxvc_ports::AudioSource>();
    assert_send::<dyn prxvc_ports::AudioSink>();
    assert_send::<dyn prxvc_ports::SystemAudioRouting>();
}
