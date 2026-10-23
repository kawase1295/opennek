mod common;

use common::{fixture, hex};
use sigfox::receiver::{ReceiverEvent, SigfoxMessage, SigfoxReceiver, estimate_first_frame_end};

const RATE: u32 = 2000;

// Streams a capture in 64-sample lines, as the streamer sends it
fn receive(raw: &[u8]) -> Vec<SigfoxMessage> {
    let mut rx = SigfoxReceiver::new(RATE);
    let mut events = vec![];
    for line in raw.chunks(64) {
        events.extend(rx.push_raw(line));
    }
    events.extend(rx.flush());
    events
        .into_iter()
        .filter_map(|e| match e {
            ReceiverEvent::Message(m) => Some(m),
            ReceiverEvent::Frame(_) => None,
        })
        .collect()
}

fn summary(m: &SigfoxMessage) -> (String, u16, String, bool, Vec<usize>) {
    (
        m.device_id.to_string(),
        m.counter,
        hex(&m.payload),
        m.downlink,
        m.ranks.clone(),
    )
}

#[test]
fn the_streaming_receiver_merges_repetitions_into_messages() {
    let raw = std::fs::read(fixture("rc3-00763DA3.rssi")).unwrap();
    let got: Vec<_> = receive(&raw).iter().map(summary).collect();
    let want: Vec<_> = (1979..=1981)
        .map(|c| ("00763DA3".into(), c, "11".into(), false, vec![0, 1, 2]))
        .collect();
    assert_eq!(got, want);
}

#[test]
fn repetitions_about_4_s_apart_still_merge_into_one_message_each() {
    let raw = std::fs::read(fixture("rc3-00763DA3-live.rssi")).unwrap();
    let got: Vec<_> = receive(&raw)
        .iter()
        .map(|m| (m.counter, hex(&m.payload), m.downlink, m.ranks.clone()))
        .collect();
    assert_eq!(
        got,
        [
            (1982, "1234567890".into(), false, vec![0, 1, 2]),
            (1983, "11".into(), false, vec![0, 1, 2]),
            (1984, "11".into(), true, vec![0, 1, 2]),
        ]
    );
}

#[test]
fn every_frame_is_reported_before_its_message() {
    let raw = std::fs::read(fixture("rc3-00763DA3.rssi")).unwrap();
    let mut rx = SigfoxReceiver::new(RATE);
    let mut frames = 0;
    for line in raw.chunks(64) {
        for e in rx.push_raw(line) {
            match e {
                ReceiverEvent::Frame(f) => {
                    assert!(f.frame.crc_ok);
                    frames += 1;
                }
                ReceiverEvent::Message(m) => assert_eq!(frames % 3, 0, "seq {} before its frames", m.counter),
            }
        }
    }
    assert_eq!(frames, 9);
}

#[test]
fn without_frame_1_its_end_is_estimated_from_frames_2_and_3() {
    let d: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("cfo-rc3-00763DA3-dl.json")).unwrap()).unwrap();
    use base64::Engine as _;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(d["raw"].as_str().unwrap())
        .unwrap();
    let run = |data: &[u8]| {
        let mut rx = SigfoxReceiver::new(RATE);
        let mut events = rx.push_raw(data);
        events.extend(rx.flush());
        events
            .into_iter()
            .filter_map(|e| match e {
                ReceiverEvent::Message(m) => Some(m),
                ReceiverEvent::Frame(_) => None,
            })
            .collect::<Vec<_>>()
    };
    let whole = &run(&raw)[0];
    assert_eq!(whole.ranks, [0, 1, 2]);
    // frame 1 (2.89-4.16 s into the recording) replaced by noise
    let mut lost = raw.clone();
    lost[(2.7 * f64::from(RATE)) as usize..(4.4 * f64::from(RATE)) as usize].fill(218);
    let m = &run(&lost)[0];
    assert_eq!(m.ranks, [1, 2]);
    assert!((m.first_frame_end.unwrap() - whole.first_frame_end.unwrap()).abs() < 0.02);
}

#[test]
fn no_estimate_without_a_later_repetition() {
    assert_eq!(estimate_first_frame_end(&[]), None);
}
