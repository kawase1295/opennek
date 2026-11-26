mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{FakeStreamer, FakeTx, api, eventually, eventually_async, soon, user_server};
use http_body_util::BodyExt;
use opennek_core::engine::{Engine, Event};
use opennek_core::host::Host;
use opennek_core::radio::{RadioEvent, RadioMessage, RadioState, now_ms};
use opennek_core::simulate::UplinkInput;
use opennek_core::store::{
    Auth, CallbackInput, CallbackSubtype, DeviceInput, DownlinkStatus, Message, ReceiverPort, Store,
};
use serde_json::{Value, json};
use sigfox::auth::{PUBLIC_KEY, uplink_auth};
use sigfox::frame::{DeviceId, UplinkFields};
use sigfox::receiver::SigfoxMessage;
use tower::ServiceExt;

const KEY: &str = "3f2a10990102030405060708090a0b0c";
const KEY_BYTES: [u8; 16] = [0x3f, 0x2a, 0x10, 0x99, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

fn received(device_id: u32, key: &[u8; 16], counter: u16, payload: &[u8], downlink: bool) -> RadioEvent {
    let f = UplinkFields {
        device_id: DeviceId(device_id),
        counter,
        payload,
        bit: None,
        downlink,
    };
    RadioEvent::Message(RadioMessage {
        message: SigfoxMessage {
            device_id: f.device_id,
            counter,
            payload: payload.to_vec(),
            bit: None,
            downlink,
            control: false,
            auth: uplink_auth(key, &f),
            time: 0.0,
            first_frame_end: None,
            rssi: -87.0,
            ranks: vec![0, 1, 2],
            frames: vec![],
            clock_error_ppm: 0,
        },
        received_at: now_ms(),
        first_frame_end_at: None,
        uplink_freq_hz: None,
    })
}

fn device(store: &Store, id: &str, body: Value) {
    store.upsert_device(&DeviceInput::parse(id, body).unwrap(), 0).unwrap();
}

fn callback(url: &str, body: &str) -> CallbackInput {
    CallbackInput::parse(json!({ "url": url, "bodyTemplate": body })).unwrap()
}

async fn simulate(engine: &Arc<Engine>, body: Value) -> Message {
    let engine = engine.clone();
    tokio::task::spawn_blocking(move || engine.simulate(&UplinkInput::parse(body).unwrap()))
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn callbacks_get_the_variables_a_received_uplink_gives_them() {
    let (url, hits) = user_server(204, "", Duration::ZERO).await;
    let store = Store::open_in_memory().unwrap();
    device(
        &store,
        "0A1B2C3D",
        json!({ "name": "kit", "key": KEY, "payloadConfig": "t::uint:8" }),
    );
    let every = "{device}|{deviceName}|{time}|{data}|{seqNumber}|{ack}|{oob}|{rssi}|{snr}|{station}|{nbFrames}|\
                 {customData#t}";
    store
        .create_callback(&callback(&format!("{url}/{{device}}"), every))
        .unwrap();
    let engine = Arc::new(Engine::new(store));
    engine.handle_radio(received(0x0A1B_2C3D, &KEY_BYTES, 7, &[0x2a, 0x01], true));
    eventually_async("the received uplink's callback", || {
        (hits.lock().unwrap().len() == 1).then_some(())
    })
    .await;
    simulate(
        &engine,
        json!({ "device": "0A1B2C3D", "data": "2a01", "ack": true, "seq": 7, "rssi": -87 }),
    )
    .await;

    let hits = hits.lock().unwrap().clone();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[1].uri, hits[0].uri);
    let vars = |body: &str| {
        let mut vars: Vec<String> = body.split('|').map(String::from).collect();
        let time = vars.remove(2);
        (vars, time)
    };
    let (simulated, time) = vars(&hits[1].body);
    assert_eq!(simulated, vars(&hits[0].body).0);
    assert_eq!(
        simulated,
        [
            "0A1B2C3D", "kit", "2a01", "7", "true", "false", "-87", "0", "0000", "3", "42"
        ]
    );
    assert!((now_ms() / 1000 - time.parse::<i64>().unwrap()).abs() < 60, "{time}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_downlink_request_shows_the_data_the_device_would_get_without_sending_it() {
    let (url, _) = user_server(
        200,
        r#"{"0B000001":{"downlinkData":"a1a2a3a4a5a6a7a8"}}"#,
        Duration::ZERO,
    )
    .await;
    let store = Store::open_in_memory().unwrap();
    let mut s = store.settings().unwrap();
    s.sigfox_downlink = true;
    store.save_settings(&s).unwrap();
    device(
        &store,
        "0A000001",
        json!({ "downlinkMode": "static", "downlinkData": "0102030405{rssi}06" }),
    );
    device(&store, "0B000001", json!({ "downlinkMode": "callback" }));
    let mut bidir = callback(&format!("{url}/"), "{}");
    bidir.callback_subtype = Some(CallbackSubtype::Bidir);
    bidir.downlink = true;
    store.create_callback(&bidir).unwrap();
    let engine = Arc::new(Engine::new(store));
    let tx = Arc::new(FakeTx::default());
    engine.set_transmitter(tx.clone());

    let downlink = |m: Message| (m.fields.downlink_status, m.fields.downlink);
    let up = json!({ "device": "0A000001", "data": "01", "ack": true, "rssi": -90 });
    assert_eq!(
        downlink(simulate(&engine, up).await),
        (DownlinkStatus::Simulated, Some("0102030405ffa606".into()))
    );
    let up = json!({ "device": "0B000001", "data": "01", "ack": true });
    assert_eq!(
        downlink(simulate(&engine, up).await),
        (DownlinkStatus::Simulated, Some("a1a2a3a4a5a6a7a8".into()))
    );
    let up = json!({ "device": "0C000001", "data": "01", "ack": true });
    assert_eq!(downlink(simulate(&engine, up).await), (DownlinkStatus::Nodata, None));
    assert!(tx.sent.lock().unwrap().is_empty());
    let logs: Vec<String> = engine.logs().into_iter().map(|l| l.line).collect();
    assert!(
        logs.iter().any(
            |l| l == "sigfox downlink for 0A000001 seq=0 skipped: the uplink was simulated (data=0102030405ffa606)"
        ),
        "{logs:?}"
    );

    let RadioEvent::Message(mut up) = received(0x0A00_0001, &PUBLIC_KEY, 1, &[1], true) else {
        unreachable!()
    };
    up.first_frame_end_at = soon();
    up.uplink_freq_hz = Some(923_183_000);
    engine.handle_radio(RadioEvent::Message(up));
    eventually_async("the received uplink's downlink", || {
        (tx.sent.lock().unwrap().len() == 1).then_some(())
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn uplinks_are_signed_with_the_registered_key_else_the_public_key() {
    let (url, hits) = user_server(204, "", Duration::ZERO).await;
    let store = Store::open_in_memory().unwrap();
    device(&store, "0A1B2C3D", json!({ "key": KEY }));
    store.create_callback(&callback(&format!("{url}/"), "{}")).unwrap();
    let engine = Arc::new(Engine::new(store));
    let auth_and_runs = async |body: Value| {
        let m = simulate(&engine, body).await;
        let runs = engine.store().list_logs(None, Some(m.id), None).unwrap().len();
        (m.fields.auth, runs)
    };
    assert_eq!(
        auth_and_runs(json!({ "device": "0a1b2c3d", "data": "01" })).await,
        (Auth::Ok, 1)
    );
    assert_eq!(
        auth_and_runs(json!({ "device": "0B000001", "data": "01" })).await,
        (Auth::Public, 1)
    );
    let public = "00112233445566778899aabbccddeeff";
    assert_eq!(
        auth_and_runs(json!({ "device": "0A1B2C3D", "data": "01", "key": public })).await,
        (Auth::Bad, 0)
    );
    assert_eq!(
        auth_and_runs(json!({ "device": "0B000001", "data": "01", "key": KEY })).await,
        (Auth::Unverified, 1)
    );
    assert_eq!(hits.lock().unwrap().len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_input_is_refused_naming_the_field() {
    let host = Host::new(Store::open_in_memory().unwrap(), Box::new(FakeStreamer::default()));
    let valid = json!({ "device": "0A1B2C3D", "data": "01" });
    for (field, bad) in [
        ("device", Value::Null),
        ("device", json!("0A1B2C3")),
        ("device", json!("0A1B2C3D0")),
        ("device", json!("0A1B2C3G")),
        ("device", json!(0x0A1B_2C3D)),
        ("data", Value::Null),
        ("data", json!("123")),
        ("data", json!("zz")),
        ("data", json!("00".repeat(13))),
        ("data", json!(1)),
        ("ack", json!("true")),
        ("ack", json!(1)),
        ("seq", json!(4096)),
        ("seq", json!(-1)),
        ("seq", json!(1.5)),
        ("seq", json!("1")),
        ("rssi", json!(1)),
        ("rssi", json!(-201)),
        ("rssi", json!(-90.5)),
        ("rssi", json!("-90")),
        ("key", json!("")),
        ("key", json!("00".repeat(15))),
        ("key", json!("zz".repeat(16))),
    ] {
        let mut body = valid.clone();
        body[field] = bad.clone();
        let (status, err) = api(&host, "POST", "/api/uplinks", Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{field}: {bad}");
        let err = err["error"].as_str().unwrap();
        assert!(err.starts_with(&format!("{field}: ")), "{field}: {bad}: {err}");
    }
    let (status, err) = api(&host, "POST", "/api/uplinks", Some(json!(["0A1B2C3D"]))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err["error"].as_str().unwrap().starts_with("body: "), "{err}");
    assert_eq!(api(&host, "GET", "/api/messages", None).await.1, json!([]));
}

#[test]
fn fields_left_out_or_null_take_their_defaults() {
    let null = json!({ "device": "0a1b2c3d", "data": "", "ack": null, "seq": null, "rssi": null, "key": null });
    assert_eq!(
        UplinkInput::parse(null).unwrap(),
        UplinkInput {
            device_id: DeviceId(0x0A1B_2C3D),
            data: vec![],
            ack: false,
            seq: None,
            rssi: -100,
            key: None,
        }
    );
    let full = json!({
        "device": "0A1B2C3D", "data": "0123456789abcdefABCDEF00", "ack": true, "seq": 4095, "rssi": -200,
        "key": KEY.to_uppercase()
    });
    assert_eq!(
        UplinkInput::parse(full).unwrap(),
        UplinkInput {
            device_id: DeviceId(0x0A1B_2C3D),
            data: vec![0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xab, 0xcd, 0xef, 0x00],
            ack: true,
            seq: Some(4095),
            rssi: -200,
            key: Some(KEY_BYTES),
        }
    );
    let edges = json!({ "device": "0A1B2C3D", "data": "", "seq": 0, "rssi": 0 });
    let edges = UplinkInput::parse(edges).unwrap();
    assert_eq!((edges.seq, edges.rssi), (Some(0), 0));
}

#[test]
fn the_seq_follows_on_from_the_devices_last_and_wraps() {
    let engine = Arc::new(Engine::new(Store::open_in_memory().unwrap()));
    let mut events = engine.subscribe();
    let seq = |body: Value| engine.simulate(&UplinkInput::parse(body).unwrap()).unwrap().fields.seq;
    let next = || seq(json!({ "device": "0A1B2C3D", "data": "01" }));
    let before = now_ms();
    assert_eq!(next(), 0);
    let added = engine.store().get_device("0A1B2C3D").unwrap().unwrap();
    assert_eq!(added.last_seq, Some(0));
    assert!(added.last_seen.unwrap() >= before);
    assert_eq!(next(), 1);
    engine.handle_radio(received(0x0A1B_2C3D, &PUBLIC_KEY, 4095, &[1], false));
    assert_eq!(next(), 0);
    assert_eq!(seq(json!({ "device": "0A1B2C3D", "data": "01", "seq": 100 })), 100);
    assert_eq!(next(), 101);
    assert_eq!(
        engine.store().get_device("0A1B2C3D").unwrap().unwrap().last_seq,
        Some(101)
    );
    device(engine.store(), "0B000001", json!({ "name": "new" }));
    assert_eq!(seq(json!({ "device": "0B000001", "data": "" })), 0);
    let live: Vec<(u16, bool)> = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|e| match e {
            Event::Message { message } => Some((message.fields.seq, message.fields.simulated)),
            _ => None,
        })
        .collect();
    assert_eq!(
        live,
        [
            (0, true),
            (1, true),
            (4095, false),
            (0, true),
            (100, true),
            (101, true),
            (0, true)
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn uplinks_are_simulated_with_or_without_a_receiver() {
    for port in [ReceiverPort::Off, ReceiverPort::Port("fake0".into())] {
        let store = Store::open_in_memory().unwrap();
        let mut s = store.settings().unwrap();
        s.sigfox_port = port.clone();
        s.sigfox_downlink = true;
        store.save_settings(&s).unwrap();
        device(
            &store,
            "0A1B2C3D",
            json!({ "downlinkMode": "static", "downlinkData": "0102030405060708" }),
        );
        let streamer = FakeStreamer::with_lines(vec!["I fw=0.3.0 f=923200000 bw=10 sr=2000 boost=1 cfo=0 pd=4".into()]);
        let host = Host::new(store, Box::new(streamer.clone()));
        host.radio.start();
        let state = match port {
            ReceiverPort::Off => RadioState::Off,
            _ => RadioState::Connected,
        };
        eventually("the receiver", || (host.radio.status().state == state).then_some(()));

        let up = json!({ "device": "0A1B2C3D", "data": "0102", "ack": true });
        let (status, m) = api(&host, "POST", "/api/uplinks", Some(up)).await;
        assert_eq!(status, StatusCode::CREATED, "{port:?}: {m}");
        assert_eq!(
            (
                m["simulated"].clone(),
                m["downlinkStatus"].clone(),
                m["downlink"].clone()
            ),
            (json!(true), json!("simulated"), json!("0102030405060708")),
            "{port:?}"
        );
        let written = streamer.written.lock().unwrap().clone();
        assert!(!written.iter().any(|l| l == "E1"), "{port:?}: {written:?}");
        assert_eq!(host.radio.status().state, state);
        host.radio.stop();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn simulated_messages_are_marked_in_the_list_and_the_csv() {
    let host = Host::new(Store::open_in_memory().unwrap(), Box::new(FakeStreamer::default()));
    host.engine
        .handle_radio(received(0x0A1B_2C3D, &PUBLIC_KEY, 1, &[1], false));
    let up = json!({ "device": "0A1B2C3D", "data": "02" });
    let (status, created) = api(&host, "POST", "/api/uplinks", Some(up)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let (_, list) = api(&host, "GET", "/api/messages", None).await;
    assert_eq!(list[0], created);
    let marks: Vec<_> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["data"].clone(), m["simulated"].clone()))
        .collect();
    assert_eq!(marks, [(json!("02"), json!(true)), (json!("01"), json!(false))]);

    let res = opennek_core::server::router(host.clone())
        .oneshot(Request::get("/api/messages.csv").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let csv = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].ends_with(",downlinkStatus,simulated"), "{csv}");
    assert!(lines[1].ends_with(",false") && lines[2].ends_with(",true"), "{csv}");
}
