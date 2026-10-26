mod common;

use std::f64::consts::PI;

use common::{Rng, hex};
use sigfox::demod::decode_capture;
use sigfox::envelope::{Complex32, Envelope, OUT_RATE};
use sigfox::frame::{DeviceId, UplinkFields, encode_uplink};

const RATE: u32 = 256_000;
const BAND: (f64, f64) = (-100_000.0, 100_000.0);

// Complex white noise with this rms per sample (Box-Muller)
fn noise(n: usize, rms: f64, rng: &mut Rng) -> Vec<Complex32> {
    (0..n)
        .map(|_| {
            let r = (-2.0 * (1.0 - rng.next()).ln()).sqrt() * rms / 2f64.sqrt();
            let a = 2.0 * PI * rng.next();
            Complex32::new((r * a.cos()) as f32, (r * a.sin()) as f32)
        })
        .collect()
}

fn add_tone(iq: &mut [Complex32], rate: u32, hz: f64, amplitude: f64) {
    for (i, v) in iq.iter_mut().enumerate() {
        let a = 2.0 * PI * hz * i as f64 / f64::from(rate);
        *v += Complex32::new((amplitude * a.cos()) as f32, (amplitude * a.sin()) as f32);
    }
}

// Pulse-shaped DBPSK at `hz`, starting at sample `at`: a data bit 0 reverses the phase on the boundary before
// it, passing through zero over +/-0.3 bit (the first bit has no reference). The device clock runs `clock_error`
// off.
fn add_frame(iq: &mut [Complex32], rate: u32, at: usize, hz: f64, amplitude: f64, bits: &[bool], clock_error: f64) {
    let per_bit = f64::from(rate) / 100.0 * (1.0 + clock_error);
    let n = (bits.len() as f64 * per_bit).ceil() as usize;
    let reversal = |k: usize| k > 0 && k < bits.len() && !bits[k];
    // The phase of each bit
    let signs: Vec<f64> = (0..bits.len())
        .scan(1.0, |s, k| {
            if reversal(k) {
                *s = -*s;
            }
            Some(*s)
        })
        .collect();
    for i in 0..n.min(iq.len() - at) {
        let t = i as f64 / per_bit;
        let k = t.round() as usize;
        let u = t - k as f64;
        let b = if reversal(k) && u.abs() < 0.3 {
            -signs[k - 1] * (PI / 2.0 * u / 0.3).sin()
        } else {
            signs[(t.floor() as usize).min(bits.len() - 1)]
        };
        let a = 2.0 * PI * hz * (at + i) as f64 / f64::from(rate) + 0.7;
        iq[at + i] += Complex32::new((amplitude * b * a.cos()) as f32, (amplitude * b * a.sin()) as f32);
    }
}

fn median(xs: &[f32]) -> f32 {
    let mut s = xs.to_vec();
    s.sort_by(f32::total_cmp);
    s[s.len() / 2]
}

#[test]
fn a_tone_in_the_band_reads_its_level_in_dbfs_at_2000_samples_per_second() {
    let mut rng = Rng::new(1);
    let mut iq = noise(RATE as usize, 0.001, &mut rng);
    add_tone(&mut iq, RATE, 41_300.0, 0.1);
    let out = Envelope::new(RATE, BAND).push(&iq);
    assert!(out.len().abs_diff(OUT_RATE as usize) < 50, "{} samples", out.len());
    let level = median(&out[100..]);
    assert!((level + 20.0).abs() < 1.0, "{level} dBFS");
}

#[test]
fn a_tone_outside_the_band_is_left_out() {
    let mut rng = Rng::new(2);
    let mut iq = noise(RATE as usize / 2, 0.001, &mut rng);
    add_tone(&mut iq, RATE, 115_000.0, 0.1);
    let out = Envelope::new(RATE, BAND).push(&iq);
    assert!(median(&out[100..]) < -60.0, "{}", median(&out[100..]));
}

#[test]
fn the_output_does_not_depend_on_how_the_samples_are_split() {
    let mut rng = Rng::new(3);
    let mut iq = noise(RATE as usize / 4, 0.01, &mut rng);
    add_tone(&mut iq, RATE, -12_345.0, 0.05);
    let whole = Envelope::new(RATE, BAND).push(&iq);
    let mut env = Envelope::new(RATE, BAND);
    let parts: Vec<f32> = iq.chunks(777).flat_map(|c| env.push(c)).collect();
    assert_eq!(whole, parts);
}

#[test]
fn moving_the_band_follows_a_tone_into_it() {
    let mut rng = Rng::new(4);
    let mut iq = noise(RATE as usize / 2, 0.001, &mut rng);
    add_tone(&mut iq, RATE, 115_000.0, 0.1);
    let mut env = Envelope::new(RATE, BAND);
    env.set_band(20_000.0, 120_000.0);
    let out = env.push(&iq);
    assert!((median(&out[100..]) + 20.0).abs() < 1.0, "{}", median(&out[100..]));
}

fn decodes_frames_from_iq(rate: u32, offsets: &[(f64, f64)]) {
    let f = UplinkFields {
        device_id: DeviceId(0x0076_3DA3),
        counter: 1234,
        payload: &[0x01, 0x02],
        bit: None,
        downlink: false,
    };
    let mut rng = Rng::new(5);
    for &(hz, clock_error) in offsets {
        for rank in 0..3 {
            let bits = encode_uplink(&f, rank, &[9, 9, 9, 9], false);
            let lead = rate as usize / 2;
            let mut iq = noise(lead * 2 + bits.len() * rate as usize / 100, 0.002, &mut rng);
            add_frame(&mut iq, rate, lead, hz, 0.05, &bits, clock_error);
            let dbfs = Envelope::new(rate, BAND).push(&iq);
            let frames = decode_capture(&dbfs, OUT_RATE, 0.0, None);
            let d = frames
                .first()
                .unwrap_or_else(|| panic!("nothing decoded at {hz} Hz, rank {rank}"));
            let frame = &d.frame;
            assert_eq!(
                (frame.device_id, frame.counter, frame.rank, frame.crc_ok),
                (DeviceId(0x0076_3DA3), 1234, rank, true),
                "{hz} Hz"
            );
            assert_eq!(hex(&frame.payload), "0102");
        }
    }
}

#[test]
fn every_repetition_of_a_frame_decodes_anywhere_in_the_band() {
    decodes_frames_from_iq(RATE, &[(-73_000.0, 0.0), (41_337.0, 0.01), (96_500.0, -0.01)]);
}

#[test]
fn a_frame_decodes_at_the_rtl_sdr_sample_rate() {
    decodes_frames_from_iq(1_024_000, &[(-55_555.0, 0.0)]);
}
