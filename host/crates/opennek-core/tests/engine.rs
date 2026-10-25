use opennek_core::engine::Engine;
use opennek_core::radio::{RadioEvent, RadioMessage};
use opennek_core::store::{MessageQuery, Store};
use sigfox::frame::DeviceId;
use sigfox::receiver::SigfoxMessage;

fn message(counter: u16, rssi: f32) -> RadioEvent {
    RadioEvent::Message(RadioMessage {
        message: SigfoxMessage {
            device_id: DeviceId(0x0076_3DA3),
            counter,
            payload: vec![0x11],
            bit: None,
            downlink: true,
            control: false,
            auth: vec![0xab, 0x6d],
            time: 0.0,
            first_frame_end: None,
            rssi,
            ranks: vec![0, 2],
            frames: vec![],
            clock_error_ppm: 0,
        },
        received_at: 1_000,
        first_frame_end_at: None,
        uplink_freq_hz: None,
    })
}

#[test]
fn rssi_is_rounded_like_the_typescript_host() {
    let engine = std::sync::Arc::new(Engine::new(Store::open_in_memory().unwrap()));
    // the streamer gives 0.5 dB steps; halves go up, as with Math.round
    engine.handle_radio(message(1, -90.5));
    engine.handle_radio(message(2, -90.4));
    let rssi: Vec<i32> = engine
        .store()
        .list_messages(&MessageQuery::default())
        .unwrap()
        .iter()
        .map(|m| m.fields.rssi)
        .collect();
    assert_eq!(rssi, [-90, -90]);
}

#[test]
fn a_late_repetition_is_not_stored_twice_and_a_downlink_request_is_answered_as_unsupported() {
    let engine = std::sync::Arc::new(Engine::new(Store::open_in_memory().unwrap()));
    let mut events = engine.subscribe();
    engine.handle_radio(message(7, -80.0));
    engine.handle_radio(message(7, -80.0));
    let messages = engine.store().list_messages(&MessageQuery::default()).unwrap();
    assert_eq!(messages.len(), 1);
    let m = &messages[0].fields;
    assert_eq!((m.nb_frames, m.ack, m.time), (2, true, 1_000));
    assert_eq!(serde_json::to_value(m.downlink_status).unwrap(), "unsupported");
    // reported as received (pending), then settled
    let statuses: Vec<_> = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|e| match e {
            opennek_core::engine::Event::Message { message } => {
                Some(serde_json::to_value(message.fields.downlink_status).unwrap())
            }
            _ => None,
        })
        .collect();
    assert_eq!(statuses, ["pending", "unsupported"]);
}

#[test]
fn an_oob_frame_that_is_not_a_control_message_is_decoded_with_the_payload_config() {
    let engine = std::sync::Arc::new(Engine::new(Store::open_in_memory().unwrap()));
    let body = serde_json::json!({ "payloadConfig": "v::uint:8" });
    engine
        .store()
        .upsert_device(&opennek_core::store::DeviceInput::parse("00763DA3", body).unwrap(), 0)
        .unwrap();
    let RadioEvent::Message(mut m) = message(3, -80.0) else {
        unreachable!()
    };
    m.message.control = true;
    m.message.downlink = false;
    engine.handle_radio(RadioEvent::Message(m));
    let stored = &engine.store().list_messages(&MessageQuery::default()).unwrap()[0].fields;
    assert_eq!(
        (stored.oob, stored.decoded.clone()),
        (true, Some(serde_json::json!({ "v": 0x11 })))
    );
}

fn console_lines(engine: &Engine) -> Vec<String> {
    engine.logs().into_iter().map(|l| l.line).collect()
}

#[test]
fn console_lines_are_kept_with_their_time() {
    let engine = Engine::new(Store::open_in_memory().unwrap());
    let before = opennek_core::radio::now_ms();
    engine.console("one".into());
    engine.console("two".into());
    let logs = engine.logs();
    assert_eq!(console_lines(&engine), ["one", "two"]);
    assert!(logs.iter().all(|l| l.time >= before));
}

#[test]
fn the_engine_keeps_the_most_recent_console_lines() {
    let engine = Engine::new(Store::open_in_memory().unwrap());
    for n in 0..=opennek_core::console::CONSOLE_LINES {
        engine.console(format!("line {n}"));
    }
    let lines = console_lines(&engine);
    assert_eq!(lines.len(), opennek_core::console::CONSOLE_LINES);
    assert_eq!(lines[0], "line 1");
}

#[test]
fn the_receiver_state_is_logged_when_it_changes() {
    use opennek_core::radio::{RadioState, RadioStatus};
    let engine = std::sync::Arc::new(Engine::new(Store::open_in_memory().unwrap()));
    let mut events = engine.subscribe();
    let connected = RadioStatus {
        state: RadioState::Connected,
        port: Some("fake0".into()),
        ..Default::default()
    };
    engine.handle_radio(RadioEvent::Status(connected.clone()));
    // a frame counted is not a change worth a line
    engine.handle_radio(RadioEvent::Status(RadioStatus {
        frames: 1,
        ..connected.clone()
    }));
    engine.handle_radio(RadioEvent::Status(RadioStatus::default()));
    assert_eq!(console_lines(&engine), ["sigfox: connected fake0", "sigfox: off"]);
    // each status is still sent to the UI, and each logged line as a console event
    let names: Vec<_> = std::iter::from_fn(|| events.try_recv().ok())
        .map(|e| e.name())
        .collect();
    assert_eq!(names, ["radio", "console", "radio", "radio", "console"]);
}
