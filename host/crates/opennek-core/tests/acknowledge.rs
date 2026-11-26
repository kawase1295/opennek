//! SERVICE ACKNOWLEDGE callbacks: what became of the downlink an uplink asked for, sent once it is settled

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::routing::any;
use opennek_core::engine::{Engine, Event};
use opennek_core::radio::{DownlinkTx, RadioEvent, RadioMessage, Transmitter, now_ms};
use opennek_core::store::{CallbackInput, DeviceInput, DownlinkStatus, Message, MessageQuery, Store};
use serde_json::{Value, json};
use sigfox::auth::{PUBLIC_KEY, uplink_auth};
use sigfox::downlink::{RC3_DL_T_RX_MS, RC3_DL_T_W_MS};
use sigfox::frame::{DeviceId, UplinkFields};
use sigfox::receiver::SigfoxMessage;

const ULF: u32 = 923_183_000;
const PRIVATE_KEY: [u8; 16] = [0x3f, 0x2a, 0x10, 0x99, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
const PRIVATE_KEY_HEX: &str = "3f2a10990102030405060708090a0b0c";
/// The body the Callbacks tab starts an ACKNOWLEDGE callback with
const ACK_BODY: &str = r#"{"device":"{device}","time":{time},"infoCode":{infoCode},"infoMessage":"{infoMessage}","downlinkAck":{downlinkAck},"downlinkOverusage":{downlinkOverusage}}"#;

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

#[derive(Clone, Debug)]
struct Hit {
    uri: String,
    method: String,
    content_type: String,
    device_header: String,
    seq_header: String,
    body: String,
}

type Hits = Arc<Mutex<Vec<Hit>>>;

/// The user's server: /bidir answers with `bidir`, anything else with 204; every request is recorded
async fn user_server(bidir: (u16, &'static str)) -> (String, Hits) {
    let hits: Hits = Arc::default();
    let seen = hits.clone();
    let app = Router::new().fallback(any(
        move |method: axum::http::Method, uri: Uri, headers: HeaderMap, body: Bytes| {
            let seen = seen.clone();
            async move {
                let header = |k: &str| headers.get(k).map_or("", |v| v.to_str().unwrap()).to_string();
                seen.lock().unwrap().push(Hit {
                    uri: uri.to_string(),
                    method: method.to_string(),
                    content_type: header("content-type"),
                    device_header: header("x-device"),
                    seq_header: header("x-seq"),
                    body: String::from_utf8_lossy(&body).into_owned(),
                });
                match uri.path() {
                    "/bidir" => (StatusCode::from_u16(bidir.0).unwrap(), bidir.1),
                    _ => (StatusCode::NO_CONTENT, ""),
                }
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, hits)
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

/// What the engine is set up with
struct Case {
    /// The device's settings, for device 0A1B2C3D
    device: Value,
    /// Send downlinks (the transmitter is there either way)
    transmit: bool,
    /// The transmitter does not send
    refuse: bool,
    /// A DATA BIDIR callback chosen for downlink, and its answer
    bidir: Option<(u16, &'static str)>,
}

fn static_data() -> Value {
    json!({ "downlinkMode": "static", "downlinkData": "0102030405060708" })
}

impl Default for Case {
    fn default() -> Self {
        Case {
            device: static_data(),
            transmit: true,
            refuse: false,
            bidir: None,
        }
    }
}

struct Setup {
    engine: Arc<Engine>,
    tx: Arc<FakeTx>,
    hits: Hits,
    ack_id: i64,
}

impl Setup {
    async fn new(case: Case) -> Setup {
        let (url, hits) = user_server(case.bidir.unwrap_or((204, ""))).await;
        let store = Store::open_in_memory().unwrap();
        let mut s = store.settings().unwrap();
        s.sigfox_downlink = case.transmit;
        s.accept_unknown_devices = true;
        store.save_settings(&s).unwrap();
        store
            .upsert_device(&DeviceInput::parse("0A1B2C3D", case.device).unwrap(), 0)
            .unwrap();
        let ack = json!({
            "callbackType": "SERVICE", "callbackSubtype": "ACKNOWLEDGE", "url": format!("{url}/ack?device={{device}}"),
            "headers": { "X-Device": "{device}", "X-Seq": "{seqNumber}" }, "bodyTemplate": ACK_BODY,
        });
        let ack_id = store.create_callback(&CallbackInput::parse(ack).unwrap()).unwrap().id;
        if case.bidir.is_some() {
            let bidir = json!({ "callbackSubtype": "BIDIR", "downlink": true, "url": format!("{url}/bidir") });
            store.create_callback(&CallbackInput::parse(bidir).unwrap()).unwrap();
        }
        let engine = Arc::new(Engine::new(store));
        let tx = Arc::new(FakeTx {
            refuse: case.refuse,
            ..Default::default()
        });
        engine.set_transmitter(tx.clone());
        Setup {
            engine,
            tx,
            hits,
            ack_id,
        }
    }

    fn acks(&self) -> Vec<Hit> {
        let hits = self.hits.lock().unwrap();
        hits.iter().filter(|h| h.uri.starts_with("/ack")).cloned().collect()
    }

    async fn ack(&self) -> Value {
        let hit = eventually("an ACKNOWLEDGE callback", || self.acks().pop()).await;
        serde_json::from_str(&hit.body).unwrap_or_else(|e| panic!("{e}: {}", hit.body))
    }

    fn message(&self, seq: u16) -> Message {
        let messages = self.engine.store().list_messages(&MessageQuery::default()).unwrap();
        messages.into_iter().find(|m| m.fields.seq == seq).unwrap()
    }

    async fn settled(&self, seq: u16) -> Message {
        eventually("a settled downlink", || {
            Some(self.message(seq)).filter(|m| m.fields.downlink_status != DownlinkStatus::Pending)
        })
        .await
    }

    /// Long enough for an ACKNOWLEDGE callback that would follow to have been sent
    async fn quiet(&self) {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

struct Up {
    key: [u8; 16],
    counter: u16,
    downlink: bool,
    first_frame_end_at: Option<i64>,
    uplink_freq_hz: Option<u32>,
}

// The device's receive window opens 100 ms after the downlink is due
fn soon() -> Option<i64> {
    Some(now_ms() - RC3_DL_T_W_MS - 2000 + 100)
}

impl Default for Up {
    fn default() -> Self {
        Up {
            key: PUBLIC_KEY,
            counter: 42,
            downlink: true,
            first_frame_end_at: soon(),
            uplink_freq_hz: Some(ULF),
        }
    }
}

fn uplink(up: Up) -> RadioEvent {
    let f = UplinkFields {
        device_id: DeviceId(0x0A1B_2C3D),
        counter: up.counter,
        payload: &[1, 2, 3],
        bit: None,
        downlink: up.downlink,
    };
    RadioEvent::Message(RadioMessage {
        message: SigfoxMessage {
            device_id: f.device_id,
            counter: up.counter,
            payload: f.payload.to_vec(),
            bit: None,
            downlink: up.downlink,
            control: false,
            auth: uplink_auth(&up.key, &f),
            time: 0.0,
            first_frame_end: Some(0.0),
            rssi: -90.0,
            ranks: vec![0, 1, 2],
            frames: vec![],
            clock_error_ppm: 0,
        },
        received_at: now_ms() - 21_000,
        first_frame_end_at: up.first_frame_end_at,
        uplink_freq_hz: up.uplink_freq_hz,
    })
}

/// What a row of the outcome table needs (the setup and the uplink), and what it gives (infoCode, infoMessage and the
/// status stored)
type Row = (&'static str, Case, Up, (u8, String), DownlinkStatus);

fn not_transmitted(why: &str) -> String {
    format!("Network error: not transmitted, {why}")
}

#[tokio::test(flavor = "multi_thread")]
async fn each_outcome_of_a_downlink_is_acknowledged_with_its_info_code() {
    let callback = || json!({ "downlinkMode": "callback" });
    let rows: Vec<Row> = vec![
        (
            "transmitted",
            Case::default(),
            Up::default(),
            (0, "Acked: the downlink was transmitted".into()),
            DownlinkStatus::Sent,
        ),
        (
            "no downlink data",
            Case {
                device: json!({ "downlinkMode": "none" }),
                ..Case::default()
            },
            Up::default(),
            (1, "No answer: no downlink data was given".into()),
            DownlinkStatus::Nodata,
        ),
        (
            "a BIDIR callback answering 204",
            Case {
                device: callback(),
                bidir: Some((204, "")),
                ..Case::default()
            },
            Up::default(),
            (1, "No answer: no downlink data was given".into()),
            DownlinkStatus::Nodata,
        ),
        (
            "downlink data that is not 8 bytes",
            Case {
                device: callback(),
                bidir: Some((200, r#"{"downlinkData":"0102"}"#)),
                ..Case::default()
            },
            Up::default(),
            (2, "Invalid payload: the downlink data is not 8 bytes of hex".into()),
            DownlinkStatus::Nodata,
        ),
        (
            "no BIDIR callback chosen",
            Case {
                device: callback(),
                ..Case::default()
            },
            Up::default(),
            (
                8,
                "Invalid configuration: the device takes its downlink from a callback, but no DATA BIDIR callback is \
                 chosen for downlink"
                    .into(),
            ),
            DownlinkStatus::Nodata,
        ),
        (
            "an unauthenticated uplink",
            Case::default(),
            Up {
                key: PRIVATE_KEY,
                ..Up::default()
            },
            (4, not_transmitted("the uplink is not authenticated")),
            DownlinkStatus::Nodata,
        ),
        (
            "receiving only",
            Case {
                transmit: false,
                ..Case::default()
            },
            Up::default(),
            (4, not_transmitted("OpenNEK is receiving only")),
            DownlinkStatus::Unsupported,
        ),
        (
            "the frequency of frame 1 unknown",
            Case::default(),
            Up {
                uplink_freq_hz: None,
                ..Up::default()
            },
            (4, not_transmitted("the time or frequency of frame 1 is unknown")),
            DownlinkStatus::Unsupported,
        ),
        (
            "a closed receive window",
            Case::default(),
            Up {
                first_frame_end_at: Some(now_ms() - RC3_DL_T_W_MS - RC3_DL_T_RX_MS),
                ..Up::default()
            },
            (4, not_transmitted("the device's receive window has closed")),
            DownlinkStatus::Failed,
        ),
        (
            "a refused transmission",
            Case {
                refuse: true,
                ..Case::default()
            },
            Up::default(),
            (4, not_transmitted("the transmitter did not send it")),
            DownlinkStatus::Failed,
        ),
    ];
    for (name, case, up, (code, info), status) in rows {
        let s = Setup::new(case).await;
        s.engine.handle_radio(uplink(up));
        let ack = s.ack().await;
        let m = s.settled(42).await;
        assert_eq!(
            ack,
            json!({
                "device": "0A1B2C3D", "time": m.fields.time / 1000, "infoCode": code, "infoMessage": info,
                "downlinkAck": code == 0, "downlinkOverusage": false,
            }),
            "{name}"
        );
        assert_eq!(m.fields.downlink_status, status, "{name}");
        assert_eq!(s.acks().len(), 1, "{name}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_acknowledge_is_sent_like_a_data_callback_with_only_its_own_variables() {
    let s = Setup::new(Case::default()).await;
    s.engine.handle_radio(uplink(Up::default()));
    s.ack().await;
    let hit = s.acks().pop().unwrap();
    assert_eq!(
        (
            hit.method.as_str(),
            hit.uri.as_str(),
            hit.content_type.as_str(),
            hit.device_header.as_str()
        ),
        ("POST", "/ack?device=0A1B2C3D", "application/json", "0A1B2C3D")
    );
    // the uplink's variables are not the Backend's for an ACKNOWLEDGE callback
    assert_eq!(hit.seq_header, "{seqNumber}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_acknowledge_is_in_the_callbacks_history_for_the_uplink_and_shown_live() {
    let s = Setup::new(Case::default()).await;
    let mut events = s.engine.subscribe();
    s.engine.handle_radio(uplink(Up::default()));
    s.ack().await;
    let message_id = s.message(42).id;
    let mut seen = vec![];
    let live = eventually("the callbackLog event", || {
        seen.extend(std::iter::from_fn(|| events.try_recv().ok()));
        seen.iter().find_map(|e| match e {
            Event::CallbackLog { log } if log.fields.callback_id == s.ack_id => Some(log.clone()),
            _ => None,
        })
    })
    .await;
    assert_eq!(
        (live.fields.message_id, live.fields.ok, live.fields.status),
        (message_id, true, Some(204))
    );
    let logs = s
        .engine
        .store()
        .list_logs(Some(s.ack_id), Some(message_id), None)
        .unwrap();
    assert_eq!(logs, [live]);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_uplink_that_does_not_ask_for_a_downlink_is_not_acknowledged() {
    let s = Setup::new(Case::default()).await;
    s.engine.handle_radio(uplink(Up {
        downlink: false,
        ..Up::default()
    }));
    assert_eq!(s.message(42).fields.downlink_status, DownlinkStatus::None);
    s.quiet().await;
    assert!(s.acks().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_frame_whose_key_does_not_match_is_not_acknowledged() {
    let s = Setup::new(Case {
        device: json!({ "key": PRIVATE_KEY_HEX, "downlinkMode": "static", "downlinkData": "0102030405060708" }),
        ..Case::default()
    })
    .await;
    // signed with the public key, while the device has a key of its own
    s.engine.handle_radio(uplink(Up::default()));
    assert_eq!(s.settled(42).await.fields.downlink_status, DownlinkStatus::Nodata);
    s.quiet().await;
    assert!(s.acks().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_acknowledge_callback_is_sent_only_for_its_devices() {
    let s = Setup::new(Case::default()).await;
    let store = s.engine.store();
    let mut cb = store.get_callback(s.ack_id).unwrap().unwrap().input;
    cb.devices = "00763DA3, F0000001".into();
    store.update_callback(s.ack_id, &cb).unwrap();
    s.engine.handle_radio(uplink(Up::default()));
    assert_eq!(s.settled(42).await.fields.downlink_status, DownlinkStatus::Sent);
    s.quiet().await;
    assert!(s.acks().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_devices_confirmation_of_a_downlink_sends_no_second_acknowledge() {
    let s = Setup::new(Case::default()).await;
    s.engine.handle_radio(uplink(Up::default()));
    assert_eq!(s.ack().await["infoCode"], 0);
    // Downlink confirmation as sigfox-ep-lib builds it, at the next counter
    let confirmation = [0x09, 0xe4, 0x0c, 0x4e, 0x0c, 0xeb, 0x00, 28];
    let f = UplinkFields {
        device_id: DeviceId(0x0A1B_2C3D),
        counter: 43,
        payload: &confirmation,
        bit: None,
        downlink: false,
    };
    let RadioEvent::Message(mut m) = uplink(Up::default()) else {
        unreachable!()
    };
    m.message = SigfoxMessage {
        counter: 43,
        payload: confirmation.to_vec(),
        downlink: false,
        control: true,
        auth: uplink_auth(&PUBLIC_KEY, &f),
        ..m.message
    };
    s.engine.handle_radio(RadioEvent::Message(m));
    assert_eq!(s.message(42).fields.downlink_status, DownlinkStatus::Acked);
    s.quiet().await;
    assert_eq!(s.acks().len(), 1);
    assert_eq!(s.tx.sent.lock().unwrap().len(), 1);
}
