//! Tests for tap format validation and deinterleaving on synthetic buffers.

use super::*;

fn fmt(flags: u32, channels: u32, bits: u32, bytes_per_frame: u32) -> RawFormat {
    RawFormat {
        sample_rate: 48_000.0,
        format_id: FORMAT_LPCM,
        format_flags: flags,
        channels,
        bits_per_channel: bits,
        bytes_per_frame,
    }
}

const FLOAT_PACKED: u32 = FLAG_IS_FLOAT | FLAG_IS_PACKED;

fn assert_unsupported(f: &RawFormat, needle: &str) {
    match StreamLayout::validate(f) {
        Err(TapError::UnsupportedFormat { detail }) => {
            assert!(detail.contains(needle), "{detail:?} lacks {needle:?}")
        }
        other => panic!("expected UnsupportedFormat, got {other:?}"),
    }
}

#[test]
fn accepts_interleaved_stereo_float32() {
    let l = StreamLayout::validate(&fmt(FLOAT_PACKED, 2, 32, 8)).unwrap();
    assert_eq!(l, StreamLayout { sample_rate: 48_000, channels: 2, non_interleaved: false });
}

#[test]
fn accepts_non_interleaved_float32() {
    let l = StreamLayout::validate(&fmt(FLOAT_PACKED | FLAG_NON_INTERLEAVED, 2, 32, 4)).unwrap();
    assert!(l.non_interleaved);
    assert_eq!(l.channels, 2);
}

#[test]
fn rejects_wrong_codec_depth_endianness_and_packing() {
    let mut f = fmt(FLOAT_PACKED, 2, 32, 8);
    f.format_id = 0x61632d33; // not 'lpcm'
    assert_unsupported(&f, "not linear PCM");
    assert_unsupported(&fmt(FLAG_IS_PACKED, 2, 32, 8), "32-bit float");
    assert_unsupported(&fmt(FLOAT_PACKED, 2, 16, 4), "32-bit float");
    assert_unsupported(&fmt(FLOAT_PACKED | FLAG_IS_BIG_ENDIAN, 2, 32, 8), "big-endian");
    assert_unsupported(&fmt(FLAG_IS_FLOAT, 2, 32, 8), "not packed");
}

#[test]
fn rejects_bad_channel_counts_rates_and_strides() {
    assert_unsupported(&fmt(FLOAT_PACKED, 0, 32, 0), "channels");
    assert_unsupported(&fmt(FLOAT_PACKED, 9, 32, 36), "channels");
    let mut f = fmt(FLOAT_PACKED, 2, 32, 8);
    f.sample_rate = 0.0;
    assert_unsupported(&f, "sample rate");
    assert_unsupported(&fmt(FLOAT_PACKED, 2, 32, 12), "bytes per frame");
}

#[test]
fn interleaved_buffer_passes_through_and_can_be_sliced() {
    let l = StreamLayout { sample_rate: 48_000, channels: 2, non_interleaved: false };
    let data = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    let bufs = [InputBuffer { channels: 2, samples: &data }];
    assert_eq!(l.frames_in(&bufs), Some(3));
    let mut out = [0.0; 4];
    l.interleave_into(&bufs, 1, 2, &mut out);
    assert_eq!(out, [3.0, 4.0, 5.0, 6.0]);
}

#[test]
fn non_interleaved_buffers_are_interleaved() {
    let l = StreamLayout { sample_rate: 48_000, channels: 2, non_interleaved: true };
    let left = [1.0, 2.0, 3.0];
    let right = [-1.0, -2.0, -3.0];
    let bufs = [
        InputBuffer { channels: 1, samples: &left },
        InputBuffer { channels: 1, samples: &right },
    ];
    assert_eq!(l.frames_in(&bufs), Some(3));
    let mut out = [0.0; 6];
    l.interleave_into(&bufs, 0, 3, &mut out);
    assert_eq!(out, [1.0, -1.0, 2.0, -2.0, 3.0, -3.0]);
}

#[test]
fn buffers_that_differ_from_the_validated_shape_are_refused() {
    let inter = StreamLayout { sample_rate: 48_000, channels: 2, non_interleaved: false };
    let data = [0.0f32; 6];
    // Wrong channel count in the buffer.
    assert_eq!(inter.frames_in(&[InputBuffer { channels: 1, samples: &data }]), None);
    // Partial frame.
    assert_eq!(inter.frames_in(&[InputBuffer { channels: 2, samples: &data[..5] }]), None);
    // No buffers.
    assert_eq!(inter.frames_in(&[]), None);

    let non = StreamLayout { sample_rate: 48_000, channels: 2, non_interleaved: true };
    // One buffer where two are expected.
    assert_eq!(non.frames_in(&[InputBuffer { channels: 1, samples: &data }]), None);
    // Unequal lengths.
    let bufs = [
        InputBuffer { channels: 1, samples: &data[..3] },
        InputBuffer { channels: 1, samples: &data[..2] },
    ];
    assert_eq!(non.frames_in(&bufs), None);
    // An interleaved buffer where planar ones are expected.
    let bufs = [
        InputBuffer { channels: 2, samples: &data },
        InputBuffer { channels: 2, samples: &data },
    ];
    assert_eq!(non.frames_in(&bufs), None);
}

#[test]
fn layout_packs_and_unpacks() {
    for l in [
        StreamLayout { sample_rate: 48_000, channels: 2, non_interleaved: false },
        StreamLayout { sample_rate: 44_100, channels: 8, non_interleaved: true },
    ] {
        assert_eq!(StreamLayout::unpack(l.pack()), Some(l));
    }
    assert_eq!(StreamLayout::unpack(0), None);
}
