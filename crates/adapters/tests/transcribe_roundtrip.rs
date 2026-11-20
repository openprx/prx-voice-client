//! Live round trip: synthesise a sentence in the owner's voice with `QwenTts`, resample it to 16 kHz
//! and recognise it with `QwenAsr`, both in one process on Metal (work package CT-11b, check e).
//! `#[ignore]`d: it needs both models. Nothing is played; the audio stays in memory.
//!
//! ```text
//! cargo test -p prxvc-adapters --features qwentts,transcribe --release --test transcribe_roundtrip \
//!     -- --ignored --nocapture --test-threads=1
//! ```
#![cfg(all(feature = "qwentts", feature = "transcribe"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod transcribe_support;

use std::time::Instant;

use prxvc_adapters::asr::{QwenAsr, QwenAsrConfig};
use prxvc_adapters::tts::qwentts::{QwenTts, QwenTtsConfig, VoiceCache, load_reference_wav};
use prxvc_audio::resample::MonoResampler;
use prxvc_media::{ClauseId, Direction, Lang, SegmentId};
use prxvc_ports::{
    AsrContext, CancellationToken, RefineAsr, SpeechSynth, TtsItem, TtsRequest, VoiceSelection,
};
use transcribe_support::{asr_model, bench_root, errors};

const EN: &str = "The meeting has been moved to Thursday afternoon, so please update your calendar before the call.";
const ZH: &str = "\u{4f1a}\u{8bae}\u{6539}\u{5230}\u{4e86}\u{661f}\u{671f}\u{56db}\u{4e0b}\u{5348}\u{ff0c}\u{8bf7}\u{5728}\u{901a}\u{8bdd}\u{4e4b}\u{524d}\u{66f4}\u{65b0}\u{4f60}\u{7684}\u{65e5}\u{5386}\u{3002}";

#[test]
#[ignore = "needs the TTS and ASR models and the GPU"]
fn live_a_synthesized_sentence_in_the_owner_voice_is_recognised() {
    let tts_dir = bench_root().join("tts/models");
    let mut cfg = QwenTtsConfig::new("qwen3-tts-1.7b-base-q8", "roundtrip-test");
    cfg.workers = 2;
    let tts = QwenTts::load_native(
        &tts_dir.join("qwen-talker-1.7b-base-Q8_0.gguf"),
        &tts_dir.join("qwen-tokenizer-12hz-Q8_0.gguf"),
        cfg,
    )
    .expect("load the synthesiser");
    let asr =
        QwenAsr::load_native(&asr_model(), QwenAsrConfig::default()).expect("load the recogniser");
    let cache_dir = tempfile::tempdir().unwrap();
    for (id, file, lang, text) in [
        ("owner-en", "en_owner.wav", Lang::En, EN),
        ("owner-zh", "zh_owner.wav", Lang::Zh, ZH),
    ] {
        let reference =
            load_reference_wav(&bench_root().join("tts/refs").join(file), id, lang, None).unwrap();
        let cache = VoiceCache::new(cache_dir.path().join(id));
        tts.prepare_voice_cached(&cache, &reference).unwrap();
        let t0 = Instant::now();
        let mut stream = tts
            .synthesize(TtsRequest {
                clause: ClauseId::new(SegmentId::first(Direction::Outgoing), 0),
                text: text.to_owned(),
                lang,
                voice: VoiceSelection::Cloned {
                    profile_id: id.to_owned(),
                },
            })
            .unwrap();
        let never = CancellationToken::new();
        let mut pcm = Vec::new();
        while let Some(item) = stream.next(&never) {
            if let TtsItem::Chunk(c) = item.unwrap() {
                pcm.extend_from_slice(&c.samples);
            }
        }
        let synth_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let mut resampler = MonoResampler::new(24_000, 16_000).unwrap();
        let mut pcm16 = resampler.convert_block(&pcm).unwrap();
        pcm16.extend(vec![0.0; 3200]);
        let t1 = Instant::now();
        let hyp = asr
            .transcribe(
                &pcm16,
                prxvc_media::SampleRate::HZ_16000,
                lang,
                &AsrContext::default(),
                &never,
            )
            .unwrap();
        let (e, n) = errors(text, &hyp.text);
        println!(
            "{id}: synthesised {:.2} s of audio in {synth_ms:.0} ms, recognised in {:.0} ms as {:?}; error {e}/{n}",
            pcm.len() as f64 / 24_000.0,
            t1.elapsed().as_secs_f64() * 1000.0,
            hyp.text
        );
        assert!(
            e as f64 / n as f64 <= 0.15,
            "round trip error too high: {e}/{n}"
        );
    }
}
