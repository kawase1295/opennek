use opennek::engine::Engine;
use opennek::radio::{RadioEvent, RadioMessage};
use opennek::store::{MessageQuery, Store};
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
    })
}

#[test]
fn rssi_is_rounded_like_the_typescript_host() {
    let engine = Engine::new(Store::open_in_memory().unwrap());
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
    let engine = Engine::new(Store::open_in_memory().unwrap());
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
            opennek::engine::Event::Message { message } => {
                Some(serde_json::to_value(message.fields.downlink_status).unwrap())
            }
            _ => None,
        })
        .collect();
    assert_eq!(statuses, ["pending", "unsupported"]);
}
