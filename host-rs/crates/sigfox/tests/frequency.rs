mod common;

use std::collections::HashMap;

use common::{dbm_of, fixture};
use sigfox::demod::decode_capture;
use sigfox::frequency::{CFO_SCALE, FrameSpan, FrequencyTracker, cfo_signed, cfo_step_hz};

const RATE: u32 = 2000;
const BASE: u32 = 923_200_000;

#[test]
fn cfo_step_follows_subgrf_get_cfo_for_the_streamer_bandwidths() {
    let close = |a: f64, b: f64| assert!((a - b).abs() < 5e-4, "{a} != {b}");
    // 234.3 kHz: demod sampling 500 kHz
    close(cfo_step_hz(0x0a, 50_000), 500_000.0 / 4096.0);
    // 4.8 kHz: 10.4 kHz sampling, oversampling 17 at 600 bps; interpolated by 4 when the bit rate is above it
    close(cfo_step_hz(0x1f, 600), 32e6 / (12.0 * 256.0) / 4096.0);
    close(cfo_step_hz(0x1f, 50_000), 32e6 / (12.0 * 256.0) * 4.0 / 4096.0);
}

#[test]
fn cfo_register_values_are_12_bit_signed() {
    assert_eq!(cfo_signed(0x7ff), 2047);
    assert_eq!(cfo_signed(0x800), -2048);
    assert_eq!(cfo_signed(0xfff), -1);
}

// A cfo-capture recording: streamer bytes, the sequence number of each 64-sample line, the CFO values per
// line and where the receiver was retuned
struct Recording {
    raw: Vec<u8>,
    rssi_seq: Vec<u64>,
    cfo: HashMap<u64, Vec<u16>>,
    tuning: Vec<(usize, u32)>,
}

fn recording(file: &str) -> Recording {
    let d: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture(file)).unwrap()).unwrap();
    use base64::Engine as _;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(d["raw"].as_str().unwrap())
        .unwrap();
    let rssi_seq = d["rssiSeq"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    let cfo = d["cfo"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| {
            (
                k.parse().unwrap(),
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|x| (x.as_i64().unwrap() & 0xfff) as u16)
                    .collect(),
            )
        })
        .collect();
    let tuning = d["tuning"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["at"].as_u64().unwrap() as usize, t["freq"].as_u64().unwrap() as u32))
        .collect();
    Recording {
        raw,
        rssi_seq,
        cfo,
        tuning,
    }
}

// Feeds the tracker line by line, as the radio does with the streamer's R, C and status lines
fn replay(rec: &Recording) -> FrequencyTracker {
    let mut t = FrequencyTracker::new(RATE, BASE, cfo_step_hz(0x0a, 50_000));
    let tuning = if rec.tuning.is_empty() {
        vec![(0, BASE)]
    } else {
        rec.tuning.clone()
    };
    for (n, seq) in rec.rssi_seq.iter().enumerate() {
        for &(at, freq) in &tuning {
            if at >= n * 64 && at < (n + 1) * 64 {
                t.tuned(freq);
            }
        }
        t.samples(&dbm_of(&rec.raw[n * 64..(n + 1) * 64]));
        if let Some(c) = rec.cfo.get(seq) {
            t.cfo(c);
        }
    }
    t
}

fn spans(rec: &Recording) -> Vec<FrameSpan> {
    decode_capture(&dbm_of(&rec.raw), RATE, 0.0, None)
        .iter()
        .map(|d| FrameSpan {
            rank: d.frame.rank,
            time: d.time,
            end: d.end,
        })
        .collect()
}

// Device 00763DA3, AT$SF=22,1 (downlink flag): frames 2 and 3 at frame 1 +/- 6 kHz
#[test]
fn frame_1_is_the_midpoint_of_frames_2_and_3_after_tuning_to_it() {
    let rec = recording("cfo-rc3-00763DA3-dl-retune.json");
    let frames = spans(&rec);
    assert_eq!(frames.iter().map(|f| f.rank).collect::<Vec<_>>(), [0, 1, 2]);
    // the same as cfo-capture --retune printed for this recording; frame 1 read directly was 122 Hz lower
    assert_eq!(replay(&rec).frame1(&frames, true), Some(923_200_488));
}

#[test]
fn frame_1_itself_is_read_against_the_frequency_it_was_retuned_to() {
    let rec = recording("cfo-rc3-00763DA3-dl-retune.json");
    let t = replay(&rec);
    let frame1 = spans(&rec).into_iter().find(|f| f.rank == 0).unwrap();
    // cfo-capture --retune: 923.200488 MHz + CFO -122 Hz / CFO_SCALE
    let f = t.frame_frequency(&frame1).unwrap();
    assert_eq!((f.lo, f.hz.round() as u32), (923_200_488, 923_200_357));
    // without frames 2 and 3, frame 1 itself is used
    assert_eq!(t.frame1(&[frame1], true), Some(923_200_357));
}

#[test]
fn without_retuning_frame_1_is_too_far_from_the_receiver_frequency_to_be_trusted() {
    // frame 1 read -43.8 kHz from 923.2 MHz: at a 7 % scale error that is ~3 kHz off
    let rec = recording("cfo-rc3-00763DA3-dl.json");
    assert_eq!(replay(&rec).frame1(&spans(&rec), true), None);
}

#[test]
fn following_the_signal_tunes_to_frame_1_while_it_is_still_on_air() {
    let rec = recording("cfo-rc3-00763DA3-dl.json");
    let mut t = FrequencyTracker::new(RATE, BASE, cfo_step_hz(0x0a, 50_000));
    t.tuned(BASE);
    let mut tunings = vec![];
    for (n, seq) in rec.rssi_seq.iter().enumerate() {
        t.samples(&dbm_of(&rec.raw[n * 64..(n + 1) * 64]));
        if let Some(c) = rec.cfo.get(seq) {
            t.cfo(c);
        }
        // the recording was not retuned, so no status line confirms the request and it stays the only one
        tunings.extend(t.next_tuning());
    }
    // the coarse frame 1 reading of that recording: 923.2 MHz - 43 823 Hz / CFO_SCALE
    assert_eq!(tunings, [923_152_878]);
}

mod tracker {
    use super::*;

    const NONE: [u32; 0] = [];

    fn feed(t: &mut FrequencyTracker, seconds: f64, dbm: f32, cfo: i32) -> Vec<u32> {
        let mut tunings = vec![];
        for _ in 0..(seconds * f64::from(RATE) / 64.0).ceil() as usize {
            t.samples(&[dbm; 64]);
            t.cfo(&[(cfo & 0xfff) as u16; 8]);
            if let Some(f) = t.next_tuning() {
                tunings.push(f);
                t.tuned(f); // the streamer confirms with a status line
            }
        }
        tunings
    }

    #[test]
    fn tunes_to_a_frame_once_then_back_to_the_base_frequency_after_the_message() {
        let mut t = FrequencyTracker::new(RATE, BASE, cfo_step_hz(0x0a, 50_000));
        t.tuned(BASE);
        assert_eq!(feed(&mut t, 2.0, -110.0, 0), NONE);
        // a frame 100 LSB (12.2 kHz) above the receiver frequency
        let step = cfo_step_hz(0x0a, 50_000);
        assert_eq!(
            feed(&mut t, 1.3, -80.0, 100),
            [BASE + (100.0 * step / CFO_SCALE).round() as u32]
        );
        // frames 2 and 3 do not move it again
        assert_eq!(feed(&mut t, 2.4, -110.0, 0), NONE);
        assert_eq!(feed(&mut t, 1.3, -80.0, 50), NONE);
        // 10 s of quiet ends the message
        assert_eq!(feed(&mut t, 11.0, -110.0, 0), [BASE]);
    }

    #[test]
    fn short_spikes_are_not_frames() {
        let mut t = FrequencyTracker::new(RATE, BASE, cfo_step_hz(0x0a, 50_000));
        t.tuned(BASE);
        feed(&mut t, 2.0, -110.0, 0);
        for _ in 0..20 {
            assert_eq!(feed(&mut t, 64.0 / f64::from(RATE), -80.0, 100), NONE);
            assert_eq!(feed(&mut t, 4.0 * 64.0 / f64::from(RATE), -110.0, 0), NONE);
        }
    }
}
