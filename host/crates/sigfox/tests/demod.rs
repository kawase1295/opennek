mod common;

use common::{Rng, dbm_of, fixture, hex};
use sigfox::demod::{burst_bits, decode_capture};
use sigfox::frame::{DeviceId, UplinkFields, encode_uplink};

const RATE: u32 = 2000;

// Pulse-shaped DBPSK envelope sampled by a receiver whose clock runs `clock_error` off
fn envelope(data_bits: &[bool], clock_error: f64, rng: &mut Rng) -> Vec<f32> {
    let (peak_dbm, floor_dbm) = (-75.0f64, -122.0f64);
    let per_bit = f64::from(RATE) / 100.0 * (1.0 + clock_error);
    let lead = (f64::from(RATE) * 0.3).round() as usize;
    let n = lead * 2 + (data_bits.len() as f64 * per_bit).ceil() as usize;
    (0..n)
        .map(|i| {
            let t = (i as f64 - lead as f64) / per_bit; // in bits
            let mut a = 0.0;
            if t >= 0.0 && t < data_bits.len() as f64 {
                // a phase reversal on the boundary before bit k when the data bit is 0 (the first bit has no
                // reference); the amplitude passes close to zero there
                let k = t.round() as usize;
                let reversal = k > 0 && k < data_bits.len() && !data_bits[k];
                a = if reversal {
                    (std::f64::consts::FRAC_PI_2 * ((t - k as f64).abs() / 0.3).min(1.0))
                        .sin()
                        .max(0.05)
                } else {
                    1.0
                };
            }
            let p = 10.0 * (10f64.powf(peak_dbm / 10.0) * a * a + 10f64.powf(floor_dbm / 10.0)).log10();
            (((p + rng.next() - 0.5) * 2.0).round() / 2.0) as f32
        })
        .collect()
}

#[test]
fn decodes_a_synthetic_frame_despite_receiver_clock_error() {
    let f = UplinkFields {
        device_id: DeviceId(0x01F9_419F),
        counter: 42,
        payload: &[1, 2, 3, 4, 5],
        bit: None,
        downlink: false,
    };
    let bits = encode_uplink(&f, 1, &[9, 9, 9, 9, 9], false);
    let mut rng = Rng::new(1);
    for clock_error in [-0.015, 0.0, 0.015] {
        let frames = decode_capture(&envelope(&bits, clock_error, &mut rng), RATE, 0.0, None);
        let d = frames
            .first()
            .unwrap_or_else(|| panic!("nothing decoded at {clock_error}"));
        let frame = &d.frame;
        assert_eq!(
            (frame.device_id, frame.counter, frame.rank, frame.crc_ok, frame.repaired),
            (DeviceId(0x01F9_419F), 42, 1, true, None),
            "{clock_error}"
        );
        assert_eq!(hex(&frame.payload), "0102030405");
        assert!(
            (f64::from(d.clock_error_ppm) - clock_error * 1e6).abs() < 3000.0,
            "{clock_error}"
        );
    }
}

#[test]
fn rejects_noise() {
    let mut rng = Rng::new(2);
    let noise: Vec<f32> = (0..20000).map(|_| (-122.0 + rng.next() * 3.0) as f32).collect();
    assert!(decode_capture(&noise, RATE, 0.0, None).is_empty());
    assert!(burst_bits(&noise[..3000], RATE).is_none());
}

// LSM100A, RC3, device 00763DA3
#[test]
fn every_repetition_of_a_real_capture_decodes_with_a_valid_crc() {
    let raw = std::fs::read(fixture("rc3-00763DA3.rssi")).unwrap();
    let frames = decode_capture(&dbm_of(&raw), RATE, 0.0, None);
    let got: Vec<_> = frames
        .iter()
        .map(|d| (d.frame.counter, d.frame.rank, d.frame.crc_ok))
        .collect();
    let want: Vec<_> = (1979..=1981).flat_map(|c| (0..3).map(move |r| (c, r, true))).collect();
    assert_eq!(got, want);
    for d in &frames {
        assert_eq!(d.frame.device_id.to_string(), "00763DA3");
        assert_eq!(hex(&d.frame.payload), "11");
    }
}

#[test]
fn a_live_session_decodes_payloads_and_the_downlink_request() {
    let raw = std::fs::read(fixture("rc3-00763DA3-live.rssi")).unwrap();
    let frames = decode_capture(&dbm_of(&raw), RATE, 0.0, None);
    let mut messages: Vec<_> = frames
        .iter()
        .map(|d| (d.frame.counter, hex(&d.frame.payload), d.frame.downlink))
        .collect();
    messages.dedup();
    assert_eq!(
        messages,
        [
            (1982, "1234567890".into(), false),
            (1983, "11".into(), false),
            (1984, "11".into(), true)
        ]
    );
}

#[test]
fn a_burst_outside_the_samples_decodes_to_nothing() {
    use sigfox::demod::{Burst, decode_burst};
    let dbm = vec![-120.0; 100];
    let burst = |start, end| Burst {
        start,
        end,
        peak_dbm: -80.0,
        floor_dbm: -120.0,
    };
    assert_eq!(decode_burst(&dbm, 1000, &burst(200, 300), 0.0), None);
    assert_eq!(decode_burst(&dbm, 1, &burst(50, 10), 0.0), None);
    assert_eq!(decode_burst(&dbm, 1000, &burst(0, usize::MAX), 0.0), None);
}
