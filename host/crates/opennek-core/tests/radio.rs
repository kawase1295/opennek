mod common;

use std::sync::{Arc, Mutex};

use common::{FakeStreamer, eventually, fixture, streamer_lines};
use opennek_core::radio::{Radio, RadioEvent, RadioSettings, RadioState};
use opennek_core::store::ReceiverPort;

fn radio(streamer: &FakeStreamer, port: &str) -> (Radio, Arc<Mutex<Vec<RadioEvent>>>) {
    let events = Arc::new(Mutex::new(vec![]));
    let sink = events.clone();
    let port = port.to_string();
    let radio = Radio::new(
        Box::new(move || RadioSettings {
            port: ReceiverPort::Port(port.clone()),
            freq: 923_200_000,
            follow: false,
        }),
        Box::new(streamer.clone()),
        Box::new(move |e| sink.lock().unwrap().push(e)),
    );
    (radio, events)
}

fn logs(events: &Mutex<Vec<RadioEvent>>) -> Vec<String> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            RadioEvent::Log(l) => Some(l.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn messages_carry_the_wall_clock_time_of_their_frames() {
    let raw = std::fs::read(fixture("rc3-00763DA3.rssi")).unwrap();
    let streamer = FakeStreamer::with_lines(streamer_lines(&raw));
    let (radio, events) = radio(&streamer, "fake0");
    radio.start();
    let messages = eventually("3 messages", || {
        let m: Vec<_> = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                RadioEvent::Message(m) => Some(m.clone()),
                _ => None,
            })
            .collect();
        (m.len() == 3).then_some(m)
    });
    radio.stop();
    // All lines arrive at once, so each message is dated back from when its third frame was decoded: three frames
    // of ~1.3 s with two 2.4 s gaps, plus the time to notice the end of the last one
    let ms = |s: f64| (s * 1000.0).round() as i64;
    let now = opennek_core::radio::now_ms();
    for m in &messages {
        let age = now - m.received_at;
        assert!((8_000..12_000).contains(&age), "seq {}: {age}", m.message.counter);
    }
    let first = &messages[0];
    let frame = first.first_frame_end_at.unwrap() - first.received_at;
    assert!(
        (frame - ms(first.message.first_frame_end.unwrap() - first.message.time)).abs() < 1000,
        "{frame}"
    );
    assert!(
        logs(&events)
            .iter()
            .any(|l| l.starts_with("sigfox: frame 1/3 00763DA3 seq=1979 CRC ok"))
    );
    let status = radio.status();
    assert_eq!(
        (status.frames, status.bad_frames, status.lost_lines, status.rate),
        (9, 0, 0, Some(2000))
    );
    assert_eq!(status.fw.as_deref(), Some("0.3.0"));
}

#[test]
fn lost_lines_are_counted_and_other_lines_logged() {
    let streamer = FakeStreamer::with_lines(
        ["R0001:F0F0", "R0004:F0F0", "hello from the bridge", "R0000:F0F0"]
            .map(String::from)
            .to_vec(),
    );
    let (radio, events) = radio(&streamer, "fake0");
    radio.start();
    eventually("lost lines", || (radio.status().lost_lines == 2 + 0xfffb).then_some(()));
    radio.stop();
    assert!(logs(&events).contains(&"sigfox: hello from the bridge".to_string()));
    assert_eq!(radio.status().rate, Some(2000));
}

#[test]
fn a_port_that_does_not_open_is_reported_and_retried() {
    let streamer = FakeStreamer::default();
    streamer.broken.lock().unwrap().push("gone".into());
    let (radio, _) = radio(&streamer, "gone");
    radio.start();
    let status = eventually("disconnected", || {
        let s = radio.status();
        (s.state == RadioState::Disconnected).then_some(s)
    });
    assert_eq!(status.error.as_deref(), Some("gone: no such device"));
    assert_eq!(status.port.as_deref(), Some("gone"));
    eventually("a retry", || (streamer.opened.lock().unwrap().len() >= 2).then_some(()));
    radio.stop();
}

#[test]
fn a_status_line_with_an_impossible_sample_rate_is_ignored() {
    let streamer = FakeStreamer::with_lines(["I fw=0.3.0 f=923200000 sr=0", "R0000:0102"].map(String::from).to_vec());
    let (radio, _) = radio(&streamer, "fake0");
    radio.start();
    eventually("connected", || {
        (radio.status().state == RadioState::Connected).then_some(())
    });
    // stop() returning shows the thread is not stuck
    radio.stop();
    assert_eq!(radio.status().rate, Some(2000));
}

#[test]
fn a_sample_that_is_not_hex_reads_as_0_and_keeps_the_line() {
    let streamer = FakeStreamer::with_lines(["R0000:01GG03", "R0001:010203"].map(String::from).to_vec());
    let (radio, _) = radio(&streamer, "fake0");
    radio.start();
    eventually("connected", || {
        (radio.status().state == RadioState::Connected).then_some(())
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    radio.stop();
    assert_eq!(radio.status().lost_lines, 0);
}

#[test]
fn a_silent_port_points_at_the_radio_firmware() {
    let streamer = FakeStreamer::default();
    let (radio, _) = radio(&streamer, "fake0");
    radio.start();
    let error = eventually("an error", || radio.status().error);
    radio.stop();
    assert!(error.contains("pio run -e radio-xiao-sx1262 -t upload"), "{error}");
    assert!(error.contains("firmware-lsm100a"), "{error}");
}

#[test]
fn a_firmware_error_is_the_radio_error_until_samples_arrive() {
    let reason = "radio not found (is the Wio-SX1262 plugged in?)";
    let streamer = FakeStreamer::with_lines(vec![format!("E {reason}\r")]);
    let (radio, events) = radio(&streamer, "fake0");
    radio.start();
    eventually("the firmware error", || {
        (radio.status().error.as_deref() == Some(reason)).then_some(())
    });
    // The stall check, which asks again every second, keeps it
    std::thread::sleep(std::time::Duration::from_millis(4500));
    let status = radio.status();
    assert_eq!(
        (status.state, status.error.as_deref()),
        (RadioState::Connecting, Some(reason))
    );
    assert!(logs(&events).contains(&format!("sigfox: {reason}")));
    streamer.lines.lock().unwrap().push_back("R0000:F0F0".into());
    eventually("connected", || {
        let s = radio.status();
        (s.state == RadioState::Connected && s.error.is_none()).then_some(())
    });
    radio.stop();
}
