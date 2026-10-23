use std::sync::{Arc, Mutex};
use std::time::Duration;

use opennek::engine::{Engine, Event};
use opennek::radio::{DownlinkTx, RadioEvent, RadioMessage, Transmitter, now_ms};
use opennek::store::{DeviceInput, DownlinkStatus, MessageQuery, Store};
use serde_json::json;
use sigfox::auth::{PUBLIC_KEY, uplink_auth};
use sigfox::downlink::{RC3_DL_T_RX_MS, RC3_DL_T_W_MS, encode_downlink};
use sigfox::frame::{DeviceId, UplinkFields};
use sigfox::receiver::SigfoxMessage;

const ULF: u32 = 923_183_000; // frame 1 somewhere in the macro channel
const PRIVATE_KEY: [u8; 16] = [0x3f, 0x2a, 0x10, 0x99, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
const DATA: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

#[derive(Default)]
struct FakeTx {
    sent: Mutex<Vec<DownlinkTx>>,
    refuse: bool,
}

impl Transmitter for FakeTx {
    fn transmit(&self, tx: &DownlinkTx) -> bool {
        self.sent.lock().unwrap().push(tx.clone());
        !self.refuse
    }
}

fn uplink(key: &[u8; 16], counter: u16, first_frame_end_at: Option<i64>, uplink_freq_hz: Option<u32>) -> RadioMessage {
    let f = UplinkFields {
        device_id: DeviceId(0x0A1B_2C3D),
        counter,
        payload: &[1, 2, 3],
        bit: None,
        downlink: true,
    };
    RadioMessage {
        message: SigfoxMessage {
            device_id: f.device_id,
            counter,
            payload: f.payload.to_vec(),
            bit: None,
            downlink: true,
            control: false,
            auth: uplink_auth(key, &f),
            time: 0.0,
            first_frame_end: Some(0.0),
            rssi: -90.0,
            ranks: vec![0, 1, 2],
            frames: vec![],
            clock_error_ppm: 0,
        },
        received_at: now_ms() - 21_000,
        first_frame_end_at,
        uplink_freq_hz,
    }
}

// The device's receive window opens 100 ms after the downlink is due
fn soon() -> Option<i64> {
    Some(now_ms() - RC3_DL_T_W_MS - 2000 + 100)
}

struct Setup {
    engine: Arc<Engine>,
    tx: Arc<FakeTx>,
    lines: Arc<Mutex<Vec<String>>>,
}

fn setup(sigfox_downlink: bool, accept_unknown_devices: bool, key: &str, refuse: bool) -> Setup {
    let store = Store::open_in_memory().unwrap();
    let mut s = store.settings().unwrap();
    s.sigfox_downlink = sigfox_downlink;
    s.accept_unknown_devices = accept_unknown_devices;
    store.save_settings(&s).unwrap();
    let device = json!({ "key": key, "downlinkMode": "static", "downlinkData": "0102030405060708" });
    store
        .upsert_device(&DeviceInput::parse("0A1B2C3D", device).unwrap(), 0)
        .unwrap();
    let engine = Arc::new(Engine::new(store));
    let tx = Arc::new(FakeTx {
        refuse,
        ..Default::default()
    });
    engine.set_transmitter(tx.clone());
    let lines = Arc::new(Mutex::new(vec![]));
    let mut events = engine.subscribe();
    let seen = lines.clone();
    std::thread::spawn(move || {
        while let Ok(e) = events.blocking_recv() {
            if let Event::Console { line, .. } = e {
                seen.lock().unwrap().push(line);
            }
        }
    });
    Setup { engine, tx, lines }
}

fn status(engine: &Engine) -> (DownlinkStatus, Option<String>) {
    let m = engine
        .store()
        .list_messages(&MessageQuery::default())
        .unwrap()
        .remove(0);
    (m.fields.downlink_status, m.fields.downlink)
}

fn settled(engine: &Engine) -> (DownlinkStatus, Option<String>) {
    for _ in 0..500 {
        let s = status(engine);
        if s.0 != DownlinkStatus::Pending {
            return s;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("still pending");
}

#[test]
fn a_public_key_device_gets_its_static_data_in_its_receive_window() {
    let s = setup(true, true, "", false);
    let up = uplink(&PUBLIC_KEY, 42, soon(), Some(ULF));
    let deadline = up.first_frame_end_at.unwrap() + RC3_DL_T_W_MS + RC3_DL_T_RX_MS - 1000;
    s.engine.handle_radio(RadioEvent::Message(up));
    // the receiver is not held up while the downlink waits for the window
    assert_eq!(status(&s.engine).0, DownlinkStatus::Pending);
    assert!(s.tx.sent.lock().unwrap().is_empty());
    assert_eq!(
        settled(&s.engine),
        (DownlinkStatus::Sent, Some("0102030405060708".into()))
    );
    let sent = s.tx.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].freq_hz, ULF - 1_000_000);
    assert_eq!(
        sent[0].frame,
        encode_downlink(DeviceId(0x0A1B_2C3D), 42, &DATA, &PUBLIC_KEY)
    );
    assert_eq!(sent[0].deadline, deadline);
    // every transmission is logged
    std::thread::sleep(Duration::from_millis(50));
    assert!(s.lines.lock().unwrap().iter().any(|l| l.contains("TX 922.183000 MHz")));
}

#[test]
fn a_registered_private_key_is_used_for_a_verified_device() {
    let s = setup(true, false, "3f2a10990102030405060708090a0b0c", false);
    s.engine
        .handle_radio(RadioEvent::Message(uplink(&PRIVATE_KEY, 7, soon(), Some(ULF))));
    assert_eq!(settled(&s.engine).0, DownlinkStatus::Sent);
    assert_eq!(
        s.tx.sent.lock().unwrap()[0].frame,
        encode_downlink(DeviceId(0x0A1B_2C3D), 7, &DATA, &PRIVATE_KEY)
    );
}

#[test]
fn nothing_is_sent_unless_transmitting_is_enabled() {
    let s = setup(false, true, "", false);
    s.engine
        .handle_radio(RadioEvent::Message(uplink(&PUBLIC_KEY, 1, soon(), Some(ULF))));
    assert_eq!(settled(&s.engine).0, DownlinkStatus::Unsupported);
    assert!(s.tx.sent.lock().unwrap().is_empty());
}

#[test]
fn an_unauthenticated_uplink_gets_no_downlink() {
    for (accept, key) in [(true, PRIVATE_KEY), (false, PUBLIC_KEY)] {
        let s = setup(true, accept, "", false);
        s.engine
            .handle_radio(RadioEvent::Message(uplink(&key, 1, soon(), Some(ULF))));
        assert_eq!(settled(&s.engine).0, DownlinkStatus::Nodata);
        assert!(s.tx.sent.lock().unwrap().is_empty());
    }
}

#[test]
fn without_the_frequency_or_the_end_of_frame_1_there_is_nothing_to_aim_at() {
    for (end, freq) in [(soon(), None), (None, Some(ULF))] {
        let s = setup(true, true, "", false);
        s.engine
            .handle_radio(RadioEvent::Message(uplink(&PUBLIC_KEY, 1, end, freq)));
        assert_eq!(settled(&s.engine).0, DownlinkStatus::Unsupported);
        assert!(s.tx.sent.lock().unwrap().is_empty());
    }
}

#[test]
fn a_closed_receive_window_or_a_refused_transmission_fails_the_downlink() {
    let late = setup(true, true, "", false);
    let closed = Some(now_ms() - RC3_DL_T_W_MS - RC3_DL_T_RX_MS);
    late.engine
        .handle_radio(RadioEvent::Message(uplink(&PUBLIC_KEY, 1, closed, Some(ULF))));
    assert_eq!(settled(&late.engine).0, DownlinkStatus::Failed);
    assert!(late.tx.sent.lock().unwrap().is_empty());

    let refused = setup(true, true, "", true);
    refused
        .engine
        .handle_radio(RadioEvent::Message(uplink(&PUBLIC_KEY, 1, soon(), Some(ULF))));
    assert_eq!(settled(&refused.engine).0, DownlinkStatus::Failed);
    assert_eq!(refused.tx.sent.lock().unwrap().len(), 1);
}

#[test]
fn a_downlink_confirmation_is_stored_as_oob_and_marks_the_downlink_as_acked() {
    // transmitting off, so that the uplinks settle at once and can be marked as sent below
    let s = setup(false, true, "", false);
    // Downlink confirmation as sigfox-ep-lib builds it: CT 0x09, idle 3300 mV, TX 3150 mV, 23.5 °C, received at -72 dBm
    let confirmation = vec![0x09, 0xe4, 0x0c, 0x4e, 0x0c, 0xeb, 0x00, 28];
    // a downlink that was sent for 2030, and one for another seq that must stay as it is
    for seq in [2030, 2029] {
        let mut up = uplink(&PUBLIC_KEY, seq, soon(), Some(ULF));
        up.message.device_id = DeviceId(0x0076_3DA3);
        up.message.auth = vec![0, 0];
        s.engine.handle_radio(RadioEvent::Message(up));
    }
    let store = s.engine.store();
    for m in store.list_messages(&MessageQuery::default()).unwrap() {
        store
            .update_downlink(m.id, Some("0102030405060708".into()), DownlinkStatus::Sent)
            .unwrap();
    }
    let control = |counter: u16, control: bool| {
        let f = UplinkFields {
            device_id: DeviceId(0x0076_3DA3),
            counter,
            payload: &confirmation,
            bit: None,
            downlink: false,
        };
        let mut m = uplink(&PUBLIC_KEY, counter, None, None);
        m.message = SigfoxMessage {
            device_id: f.device_id,
            payload: confirmation.clone(),
            downlink: false,
            control,
            auth: uplink_auth(&PUBLIC_KEY, &f),
            ..m.message
        };
        RadioEvent::Message(m)
    };
    s.engine.handle_radio(control(2031, true));
    let messages = store
        .list_messages(&MessageQuery {
            device_id: Some("00763DA3".into()),
            ..Default::default()
        })
        .unwrap();
    let by_seq = |seq: u16| messages.iter().find(|m| m.fields.seq == seq).unwrap().fields.clone();
    let ack = by_seq(2031);
    assert_eq!((ack.oob, ack.ack, ack.data.as_str()), (true, false, "09e40c4e0ceb001c"));
    assert_eq!(ack.decoded.unwrap()["downlinkRssiDbm"], -72);
    assert_eq!(by_seq(2030).downlink_status, DownlinkStatus::Acked);
    assert_eq!(by_seq(2029).downlink_status, DownlinkStatus::Sent);
}
