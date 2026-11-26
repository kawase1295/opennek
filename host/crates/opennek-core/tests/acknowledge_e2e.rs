//! SERVICE ACKNOWLEDGE callbacks end to end: uplinks from the receiver (the fake one), a DATA BIDIR callback that
//! answers with downlink data or not, the downlink through the receiver, and the ACKNOWLEDGE callback that reports it

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::http::{StatusCode, Uri};
use axum::routing::any;
use common::{FakeStreamer, cfo_streamer_lines};
use opennek_core::host::Host;
use opennek_core::radio::{RadioEvent, RadioMessage, RadioState, now_ms};
use opennek_core::store::{CallbackInput, DeviceInput, Message, MessageQuery, ReceiverPort, Store};
use serde_json::{Value, json};
use sigfox::auth::{PUBLIC_KEY, uplink_auth};
use sigfox::downlink::{RC3_DL_T_W_MS, encode_downlink};
use sigfox::frame::{DeviceId, UplinkFields};
use sigfox::receiver::SigfoxMessage;

const STATUS_LINE: &str = "I fw=0.3.0 f=923200000 bw=10 sr=2000 boost=1 cfo=1 pd=4";
const ACK_BODY: &str = r#"{"device":"{device}","time":{time},"infoCode":{infoCode},"infoMessage":"{infoMessage}","downlinkAck":{downlinkAck},"downlinkOverusage":{downlinkOverusage}}"#;
const DOWNLINK_DATA: &str = r#"{"00763DA3":{"downlinkData":"0102030405060708"}}"#;

type Bodies = Arc<Mutex<Vec<(String, String)>>>;

/// The user's server: the DATA BIDIR callback (/bidir) answers with `bidir`, the ACKNOWLEDGE one (/ack) with 204;
/// each request's path and body are recorded
async fn user_server(bidir: (u16, &'static str)) -> (String, Bodies) {
    let bodies: Bodies = Arc::default();
    let seen = bodies.clone();
    let app = Router::new().fallback(any(move |uri: Uri, body: Bytes| {
        let seen = seen.clone();
        async move {
            let body = String::from_utf8_lossy(&body).into_owned();
            seen.lock().unwrap().push((uri.path().to_string(), body));
            match uri.path() {
                "/bidir" => (StatusCode::from_u16(bidir.0).unwrap(), bidir.1),
                _ => (StatusCode::NO_CONTENT, ""),
            }
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, bodies)
}

async fn eventually<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    for _ in 0..1000 {
        if let Some(v) = f() {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

struct Setup {
    host: Arc<Host>,
    streamer: FakeStreamer,
    bodies: Bodies,
}

/// A host on a receiver with firmware 0.3.0 that sends `lines` and transmits whatever it is asked to, sending
/// downlinks; device 00763DA3 takes its downlink from a chosen DATA BIDIR callback that answers with `bidir`, and an
/// ACKNOWLEDGE callback reports what became of it
async fn setup(lines: Vec<String>, bidir: (u16, &'static str)) -> Setup {
    let (url, bodies) = user_server(bidir).await;
    let store = Store::open_in_memory().unwrap();
    let mut s = store.settings().unwrap();
    s.sigfox_port = ReceiverPort::Port("fake0".into());
    s.sigfox_downlink = true;
    store.save_settings(&s).unwrap();
    let device = DeviceInput::parse("00763DA3", json!({ "downlinkMode": "callback" })).unwrap();
    store.upsert_device(&device, 0).unwrap();
    let bidir = json!({ "callbackSubtype": "BIDIR", "downlink": true, "url": format!("{url}/bidir") });
    store.create_callback(&CallbackInput::parse(bidir).unwrap()).unwrap();
    let ack = json!({ "callbackType": "SERVICE", "callbackSubtype": "ACKNOWLEDGE", "url": format!("{url}/ack"),
        "bodyTemplate": ACK_BODY });
    store.create_callback(&CallbackInput::parse(ack).unwrap()).unwrap();
    let mut lines = lines;
    lines.insert(0, STATUS_LINE.into());
    let streamer = FakeStreamer::with_lines(lines);
    *streamer.answer.lock().unwrap() = Some(Box::new(|cmd: &str| {
        cmd.starts_with('T').then(|| "D ok ms=281".to_string())
    }));
    let host = Host::new(store, Box::new(streamer.clone()));
    host.radio.start();
    Setup { host, streamer, bodies }
}

impl Setup {
    /// The body of the ACKNOWLEDGE callback, once it is sent
    async fn acknowledge(&self) -> Value {
        let body = eventually("the ACKNOWLEDGE callback", || {
            let bodies = self.bodies.lock().unwrap();
            bodies.iter().find(|(path, _)| path == "/ack").map(|(_, b)| b.clone())
        })
        .await;
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("{e}: {body}"))
    }

    fn message(&self, seq: u16) -> Message {
        let messages = self
            .host
            .engine
            .store()
            .list_messages(&MessageQuery::default())
            .unwrap();
        messages.into_iter().find(|m| m.fields.seq == seq).unwrap()
    }

    fn transmitted(&self) -> Vec<String> {
        let written = self.streamer.written.lock().unwrap();
        written.iter().filter(|c| c.starts_with('T')).cloned().collect()
    }
}

/// The recorded capture of device 00763DA3 sending seq 2021 with the downlink flag (AT$SF=22,1)
fn recorded_downlink_request() -> Vec<String> {
    cfo_streamer_lines("cfo-rc3-00763DA3-dl-retune.json")
}

// Answered with no data, the request is NO_ANSWER. Answered with data, nothing goes on air: the recordings of fixtures/
// are signed with the device's private key, which OpenNEK does not have, so the uplink reads as unverified and no
// downlink can be signed for it
#[tokio::test(flavor = "multi_thread")]
async fn a_recorded_downlink_request_is_acknowledged_with_why_nothing_was_sent() {
    let no_data = setup(recorded_downlink_request(), (204, "")).await;
    let given = setup(recorded_downlink_request(), (200, DOWNLINK_DATA)).await;
    let (no_data_ack, given_ack) = (no_data.acknowledge().await, given.acknowledge().await);
    no_data.host.radio.stop();
    given.host.radio.stop();

    let m = no_data.message(2021);
    assert_eq!(
        (m.fields.ack, serde_json::to_value(m.fields.auth).unwrap()),
        (true, json!("unverified"))
    );
    assert_eq!(
        no_data_ack,
        json!({
            "device": "00763DA3", "time": m.fields.time / 1000, "infoCode": 1,
            "infoMessage": "No answer: no downlink data was given", "downlinkAck": false, "downlinkOverusage": false,
        })
    );
    assert_eq!(serde_json::to_value(m.fields.downlink_status).unwrap(), "nodata");

    let m = given.message(2021);
    assert_eq!(
        given_ack,
        json!({
            "device": "00763DA3", "time": m.fields.time / 1000, "infoCode": 4,
            "infoMessage": "Network error: not transmitted, the uplink is not authenticated", "downlinkAck": false,
            "downlinkOverusage": false,
        })
    );
    assert_eq!(
        (
            serde_json::to_value(m.fields.downlink_status).unwrap(),
            m.fields.downlink
        ),
        (json!("nodata"), Some("0102030405060708".into()))
    );
    assert!(given.transmitted().is_empty());
}

// No recording can be authenticated (see above), so this uplink, signed with the public key, is handed to the engine as
// the receiver would hand it; its downlink goes out through the receiver
#[tokio::test(flavor = "multi_thread")]
async fn a_downlink_sent_through_the_receiver_is_acknowledged_as_acked() {
    let s = setup(vec![], (200, DOWNLINK_DATA)).await;
    eventually("the receiver", || {
        let status = s.host.radio.status();
        (status.state == RadioState::Connected && status.fw.is_some()).then_some(())
    })
    .await;
    let f = UplinkFields {
        device_id: DeviceId(0x0076_3DA3),
        counter: 2022,
        payload: &[0x22],
        bit: None,
        downlink: true,
    };
    let uplink = RadioMessage {
        message: SigfoxMessage {
            device_id: f.device_id,
            counter: f.counter,
            payload: f.payload.to_vec(),
            bit: None,
            downlink: true,
            control: false,
            auth: uplink_auth(&PUBLIC_KEY, &f),
            time: 0.0,
            first_frame_end: Some(0.0),
            rssi: -90.0,
            ranks: vec![0, 1, 2],
            frames: vec![],
            clock_error_ppm: 0,
        },
        received_at: now_ms() - 21_000,
        // the device's receive window opens 100 ms after the downlink is due
        first_frame_end_at: Some(now_ms() - RC3_DL_T_W_MS - 2000 + 100),
        uplink_freq_hz: Some(923_200_488),
    };
    s.host.engine.handle_radio(RadioEvent::Message(uplink));
    let ack = s.acknowledge().await;
    s.host.radio.stop();

    let m = s.message(2022);
    assert_eq!(
        ack,
        json!({
            "device": "00763DA3", "time": m.fields.time / 1000, "infoCode": 0,
            "infoMessage": "Acked: the downlink was transmitted", "downlinkAck": true, "downlinkOverusage": false,
        })
    );
    let frame = encode_downlink(f.device_id, 2022, &[1, 2, 3, 4, 5, 6, 7, 8], &PUBLIC_KEY);
    let hex: String = frame.iter().map(|b| format!("{b:02X}")).collect();
    assert_eq!(s.transmitted(), [format!("T922200488,10,{hex}")]);
    assert_eq!(serde_json::to_value(m.fields.downlink_status).unwrap(), "sent");
}
