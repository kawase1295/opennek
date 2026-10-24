mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{FakeStreamer, cfo_streamer_lines, eventually};
use opennek::radio::{DownlinkTx, Radio, RadioEvent, RadioSettings, RadioState, Transmitter, now_ms};
use sigfox::frame::DeviceId;

const FRAME: [u8; 15] = [
    0xd1, 0x23, 0x2e, 0xc8, 0x8a, 0x6a, 0x34, 0x71, 0x8a, 0xb5, 0xd1, 0x50, 0x88, 0x37, 0xe4,
];

fn downlink(deadline_in_ms: i64) -> DownlinkTx {
    DownlinkTx {
        device_id: DeviceId(0x0A1B_2C3D),
        counter: 1234,
        freq_hz: 922_183_000,
        frame: FRAME,
        deadline: now_ms() + deadline_in_ms,
    }
}

struct Setup {
    radio: Arc<Radio>,
    streamer: FakeStreamer,
    events: Arc<Mutex<Vec<RadioEvent>>>,
}

impl Setup {
    fn logs(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                RadioEvent::Log(l) => Some(l.clone()),
                _ => None,
            })
            .collect()
    }

    fn commands(&self) -> Vec<String> {
        self.streamer.written.lock().unwrap().clone()
    }
}

// A streamer with the given firmware that answers T commands with `answer`
fn setup(
    fw: &str,
    follow: bool,
    lines: Vec<String>,
    answer: impl FnMut(&str) -> Option<String> + Send + 'static,
) -> Setup {
    let mut lines = lines;
    lines.insert(0, format!("I fw={fw} f=923200000 bw=10 sr=2000 boost=1 cfo=1 pd=4"));
    let streamer = FakeStreamer::with_lines(lines);
    let mut answer = answer;
    *streamer.answer.lock().unwrap() = Some(Box::new(move |cmd: &str| {
        if cmd.starts_with('T') {
            answer(cmd).map(|r| format!("D {r}"))
        } else {
            None
        }
    }));
    let events = Arc::new(Mutex::new(vec![]));
    let sink = events.clone();
    let radio = Arc::new(Radio::new(
        Box::new(move || RadioSettings {
            port: "fake0".into(),
            freq: 923_200_000,
            follow,
        }),
        Box::new(streamer.clone()),
        Box::new(move |e| sink.lock().unwrap().push(e)),
    ));
    radio.start();
    eventually("connected", || {
        (radio.status().state == RadioState::Connected).then_some(())
    });
    Setup {
        radio,
        streamer,
        events,
    }
}

#[test]
fn the_receiver_is_tuned_and_asked_for_cfo_lines_and_its_status() {
    let s = setup("0.3.0", false, vec![], |_| None);
    eventually("commands", || (s.commands().len() >= 3).then_some(()));
    assert_eq!(s.commands()[..3], ["F923200000", "C1", "?"]);
    s.radio.stop();
}

#[test]
fn a_downlink_is_allowed_sent_and_disallowed_again() {
    let s = setup("0.3.0", false, vec![], |_| Some("ok ms=281".into()));
    assert!(s.radio.transmit(&downlink(5000)));
    let commands = s.commands();
    let from = commands.iter().position(|c| c == "E1").unwrap();
    assert_eq!(
        commands[from..],
        ["E1", "T922183000,10,D1232EC88A6A34718AB5D1508837E4", "E0"]
    );
    assert!(
        s.logs()
            .contains(&"sigfox: TX 922.183000 MHz 10 dBm: ok ms=281".to_string())
    );
    s.radio.stop();
}

#[test]
fn a_busy_channel_is_tried_again_until_it_is_free() {
    let mut n = 0;
    let s = setup("0.3.0", false, vec![], move |_| {
        n += 1;
        Some(if n < 3 { "busy rssi=-71.5" } else { "ok ms=281" }.into())
    });
    assert!(s.radio.transmit(&downlink(5000)));
    assert_eq!(s.commands().iter().filter(|c| c.starts_with('T')).count(), 3);
    s.radio.stop();
}

#[test]
fn a_busy_channel_is_given_up_at_the_deadline() {
    let s = setup("0.3.0", false, vec![], |_| Some("busy rssi=-60.0".into()));
    assert!(!s.radio.transmit(&downlink(500)));
    assert_eq!(s.commands().last().unwrap(), "E0");
    assert_eq!(
        s.logs().last().unwrap(),
        "sigfox: TX not sent: the device's receive window has closed"
    );
    s.radio.stop();
}

#[test]
fn a_refusal_from_the_firmware_is_not_retried() {
    let s = setup("0.3.0", false, vec![], |_| Some("range".into()));
    assert!(!s.radio.transmit(&downlink(5000)));
    assert_eq!(s.commands().iter().filter(|c| c.starts_with('T')).count(), 1);
    s.radio.stop();
}

#[test]
fn no_reply_counts_as_not_sent() {
    let s = setup("0.3.0", false, vec![], |_| None);
    assert!(!s.radio.transmit(&downlink(5000)));
    s.radio.stop();
}

#[test]
fn an_older_streamer_is_not_asked_to_transmit() {
    let s = setup("0.2.0", false, vec![], |_| Some("ok".into()));
    assert!(!s.radio.transmit(&downlink(5000)));
    assert!(!s.commands().iter().any(|c| c.starts_with('T') || c.starts_with('E')));
    assert!(
        s.logs()
            .last()
            .unwrap()
            .contains("streamer firmware 0.3.0 or later needed (fw=0.2.0)")
    );
    s.radio.stop();
}

#[test]
fn without_a_receiver_nothing_is_sent() {
    let radio = Radio::new(
        Box::new(|| RadioSettings {
            port: String::new(),
            freq: 923_200_000,
            follow: false,
        }),
        Box::new(FakeStreamer::default()),
        Box::new(|_| {}),
    );
    assert!(!radio.transmit(&downlink(5000)));
}

fn messages(s: &Setup) -> Vec<opennek::radio::RadioMessage> {
    s.events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            RadioEvent::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect()
}

// Device 00763DA3, AT$SF=22,1 (downlink flag): frames 2 and 3 at frame 1 +/- 6 kHz
#[test]
fn frame_1_is_the_midpoint_of_frames_2_and_3_after_tuning_to_it() {
    let s = setup(
        "0.2.0",
        false,
        cfo_streamer_lines("cfo-rc3-00763DA3-dl-retune.json"),
        |_| None,
    );
    let m = eventually("the message", || messages(&s).pop());
    s.radio.stop();
    assert_eq!(
        (m.message.counter, m.message.downlink, m.message.ranks.clone()),
        (2021, true, vec![0, 1, 2])
    );
    // the same as cfo-capture --retune printed for this recording
    assert_eq!(m.uplink_freq_hz, Some(923_200_488));
    assert!(
        s.logs()
            .iter()
            .any(|l| l.starts_with("sigfox: frame 1 of 00763DA3 seq=2021 at 923.200488 MHz (1: "))
    );
}

#[test]
fn without_retuning_frame_1_is_too_far_from_the_receiver_frequency_to_be_trusted() {
    let s = setup("0.2.0", false, cfo_streamer_lines("cfo-rc3-00763DA3-dl.json"), |_| None);
    // the message waits for its quiet period after the last frame; stopping flushes it
    std::thread::sleep(Duration::from_millis(500));
    s.radio.stop();
    let m = messages(&s);
    assert_eq!(
        m.iter()
            .map(|m| (m.message.counter, m.uplink_freq_hz))
            .collect::<Vec<_>>(),
        [(2020, None)]
    );
}

#[test]
fn following_the_signal_tunes_to_frame_1_while_it_is_still_on_air() {
    let s = setup("0.2.0", true, cfo_streamer_lines("cfo-rc3-00763DA3-dl.json"), |_| None);
    eventually("a tuning", || {
        s.logs().iter().any(|l| l.contains("tuning to")).then_some(())
    });
    std::thread::sleep(Duration::from_millis(500));
    s.radio.stop();
    let tunings: Vec<String> = s.logs().into_iter().filter(|l| l.contains("tuning to")).collect();
    // the coarse frame 1 reading of that recording: 923.2 MHz - 43 823 Hz / CFO_SCALE
    assert_eq!(tunings, ["sigfox: tuning to 923.152878 MHz"]);
    assert!(s.commands().contains(&"F923152878".to_string()));
}

#[test]
fn a_pre_release_firmware_is_not_taken_for_0_3_0() {
    let s = setup("0.3.0-rc1", false, vec![], |_| Some("ok".into()));
    assert!(!s.radio.transmit(&downlink(5000)));
    assert!(
        s.logs()
            .last()
            .unwrap()
            .contains("streamer firmware 0.3.0 or later needed (fw=0.3.0-rc1)")
    );
    s.radio.stop();
}

#[test]
fn a_cfo_line_with_a_garbled_sequence_number_is_dropped_quietly() {
    let s = setup("0.3.0", false, vec!["C00G1:000".into()], |_| None);
    std::thread::sleep(Duration::from_millis(100));
    s.radio.stop();
    assert!(!s.logs().iter().any(|l| l.contains("C00G1")), "{:?}", s.logs());
}
