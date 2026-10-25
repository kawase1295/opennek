mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{FakeStreamer, eventually, fixture, streamer_lines};
use http_body_util::BodyExt;
use opennek_core::host::Host;
use opennek_core::store::Store;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn get(host: &Arc<Host>, path: &str) -> (StatusCode, String, String) {
    let res = opennek_core::server::router(host.clone())
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let content_type = res
        .headers()
        .get("content-type")
        .map_or("", |v| v.to_str().unwrap())
        .to_string();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, content_type, String::from_utf8_lossy(&body).into_owned())
}

async fn get_json(host: &Arc<Host>, path: &str) -> Value {
    let (status, _, body) = get(host, path).await;
    assert_eq!(status, StatusCode::OK, "{path}: {body}");
    serde_json::from_str(&body).unwrap()
}

fn host_with_capture(file: &str) -> Arc<Host> {
    let store = Store::open_in_memory().unwrap();
    let mut s = store.settings().unwrap();
    s.sigfox_port = "fake0".into();
    store.save_settings(&s).unwrap();
    let raw = std::fs::read(fixture(file)).unwrap();
    let host = Host::new(store, Box::new(FakeStreamer::with_lines(streamer_lines(&raw))));
    host.radio.start();
    host
}

#[tokio::test(flavor = "multi_thread")]
async fn uplinks_from_the_receiver_are_served_to_the_ui() {
    let host = host_with_capture("rc3-00763DA3.rssi");
    let messages = {
        let mut found = None;
        for _ in 0..1000 {
            let m = get_json(&host, "/api/messages").await;
            if m.as_array().unwrap().len() == 3 {
                found = Some(m);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        found.expect("3 messages")
    };
    let summary: Vec<_> = messages
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            json!({ "deviceId": m["deviceId"], "seq": m["seq"], "data": m["data"], "nbFrames": m["nbFrames"],
                "auth": m["auth"], "ack": m["ack"], "oob": m["oob"], "downlinkStatus": m["downlinkStatus"] })
        })
        .collect();
    let want: Vec<_> = [1981, 1980, 1979]
        .iter()
        .map(|seq| {
            json!({ "deviceId": "00763DA3", "seq": seq, "data": "11", "nbFrames": 3, "auth": "unverified",
                "ack": false, "oob": false, "downlinkStatus": "none" })
        })
        .collect();
    assert_eq!(summary, want);
    // received at about now, not at the stream time
    let now = chrono::Utc::now().timestamp_millis();
    assert!((now - messages[0]["time"].as_i64().unwrap()).abs() < 60_000);

    let devices = get_json(&host, "/api/devices").await;
    assert_eq!(
        (devices[0]["id"].clone(), devices[0]["lastSeq"].clone()),
        (json!("00763DA3"), json!(1981))
    );

    let status = get_json(&host, "/api/status").await;
    assert_eq!(status["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(status["radio"]["state"], "connected");
    assert_eq!(status["radio"]["port"], "fake0");
    assert_eq!(status["radio"]["frames"], 9);
    assert_eq!(status["settings"]["sigfoxPort"], "fake0");
    host.radio.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_ui_is_served_from_the_binary() {
    let host = Host::new(Store::open_in_memory().unwrap(), Box::new(FakeStreamer::default()));
    let (status, content_type, body) = get(&host, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/html"));
    assert!(body.contains(r#"<script type="module" src="/app.js">"#));
    for (path, kind) in [
        ("/app.js", "text/javascript"),
        ("/i18n.js", "text/javascript"),
        ("/style.css", "text/css"),
    ] {
        let (status, content_type, body) = get(&host, path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(content_type.starts_with(kind), "{path}: {content_type}");
        assert!(!body.is_empty());
    }
    // what the UI loads before it subscribes to events
    for path in [
        "/api/status",
        "/api/devices",
        "/api/callbacks",
        "/api/messages",
        "/api/settings",
        "/api/callback-logs",
        "/api/logs",
    ] {
        get_json(&host, path).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_log_is_served_oldest_first() {
    let host = Host::new(Store::open_in_memory().unwrap(), Box::new(FakeStreamer::default()));
    host.engine.console("first".into());
    host.engine.console("second".into());
    let logs = get_json(&host, "/api/logs").await;
    let lines: Vec<_> = logs.as_array().unwrap().iter().map(|l| l["line"].clone()).collect();
    assert_eq!(lines, [json!("first"), json!("second")]);
    let now = chrono::Utc::now().timestamp_millis();
    assert!((now - logs[0]["time"].as_i64().unwrap()).abs() < 60_000);
}

#[tokio::test(flavor = "multi_thread")]
async fn changing_the_port_reconnects_the_receiver() {
    let streamer = FakeStreamer::default();
    let host = Host::new(Store::open_in_memory().unwrap(), Box::new(streamer.clone()));
    host.radio.start();
    eventually("off", || {
        (host.radio.status().state == opennek_core::radio::RadioState::Off).then_some(())
    });

    let res = opennek_core::server::router(host.clone())
        .oneshot(
            Request::put("/api/settings")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"sigfoxPort":"fake1"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    // A change that lands between the radio reading the generation and the settings opens the port twice
    eventually("fake1 opened", || {
        (streamer.opened.lock().unwrap().last().map(String::as_str) == Some("fake1")).then_some(())
    });
    // tuned to the saved frequency and asked for its status
    eventually("commands", || {
        let w = streamer.written.lock().unwrap().clone();
        w.ends_with(&["F923200000".to_string(), "C1".into(), "?".into()])
            .then_some(())
    });

    let res = opennek_core::server::router(host.clone())
        .oneshot(
            Request::put("/api/settings")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"sigfoxFreq":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(body["error"].as_str().unwrap().contains("sigfoxFreq"));
    host.radio.stop();
}

async fn send(host: &Arc<Host>, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    let body = body.map_or(Body::empty(), |b| Body::from(b.to_string()));
    let res = opennek_core::server::router(host.clone())
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn wait_for_messages(host: &Arc<Host>, n: usize) -> Value {
    for _ in 0..1000 {
        let m = get_json(host, "/api/messages").await;
        if m.as_array().unwrap().len() == n {
            return m;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("{n} messages expected");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_registered_device_gets_its_uplinks_decoded() {
    let store = Store::open_in_memory().unwrap();
    let host = Host::new(store, Box::new(FakeStreamer::default()));
    let (status, device) = send(
        &host,
        "PUT",
        "/api/devices/00763da3",
        Some(json!({ "name": "kit", "payloadConfig": "v::uint:4 w::uint:4" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{device}");
    assert_eq!(
        (device["id"].clone(), device["name"].clone()),
        (json!("00763DA3"), json!("kit"))
    );
    let (status, err) = send(&host, "PUT", "/api/devices/00763DA3", Some(json!({ "key": "12" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err["error"].as_str().unwrap().starts_with("key:"));

    // the recorded uplinks carry 0x11; a new host so that the device is configured before they arrive
    let raw = std::fs::read(fixture("rc3-00763DA3.rssi")).unwrap();
    let store = Store::open_in_memory().unwrap();
    let mut s = store.settings().unwrap();
    s.sigfox_port = "fake0".into();
    store.save_settings(&s).unwrap();
    let host = Host::new(store, Box::new(FakeStreamer::with_lines(streamer_lines(&raw))));
    send(
        &host,
        "PUT",
        "/api/devices/00763DA3",
        Some(json!({ "payloadConfig": "v::uint:4 w::uint:4" })),
    )
    .await;
    host.radio.start();
    let messages = wait_for_messages(&host, 3).await;
    host.radio.stop();
    assert_eq!(messages[0]["decoded"], json!({ "v": 1, "w": 1 }));
    // the configuration was kept when the device was seen on air
    let devices = get_json(&host, "/api/devices").await;
    assert_eq!(devices[0]["payloadConfig"], "v::uint:4 w::uint:4");

    let (status, _, csv) = get(&host, "/api/messages.csv?device=00763da3").await;
    assert_eq!(status, StatusCode::OK);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        lines[0],
        "id,time,deviceId,seq,data,decoded,ack,oob,rssi,nbFrames,auth,downlink,downlinkStatus"
    );
    assert_eq!(lines.len(), 4);
    let cells: Vec<&str> = lines[1].splitn(6, ',').collect();
    assert_eq!(cells[2..5], ["00763DA3", "1981", "11"]);
    assert!(cells[1].ends_with('Z') && cells[1].len() == 24, "{}", cells[1]);
    assert!(
        cells[5].starts_with(r#""{""v"":1,""w"":1}",false,false,"#),
        "{}",
        cells[5]
    );

    let (status, _) = send(&host, "DELETE", "/api/messages?device=0A1B2C3D", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(get_json(&host, "/api/messages").await.as_array().unwrap().len(), 3);
    send(&host, "DELETE", "/api/messages?device=00763da3", None).await;
    assert_eq!(get_json(&host, "/api/messages").await, json!([]));
    let (status, _) = send(&host, "DELETE", "/api/devices/00763da3", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(get_json(&host, "/api/devices").await, json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_payload_that_does_not_fit_its_config_is_stored_with_the_error() {
    let raw = std::fs::read(fixture("rc3-00763DA3.rssi")).unwrap();
    let store = Store::open_in_memory().unwrap();
    let mut s = store.settings().unwrap();
    s.sigfox_port = "fake0".into();
    store.save_settings(&s).unwrap();
    let host = Host::new(store, Box::new(FakeStreamer::with_lines(streamer_lines(&raw))));
    send(
        &host,
        "PUT",
        "/api/devices/00763DA3",
        Some(json!({ "payloadConfig": "x::uint:16" })),
    )
    .await;
    host.radio.start();
    let messages = wait_for_messages(&host, 3).await;
    host.radio.stop();
    assert!(
        messages[0]["decoded"]["_error"]
            .as_str()
            .unwrap()
            .contains("exceeds payload")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_ui_language_is_kept_in_the_settings() {
    let host = Host::new(Store::open_in_memory().unwrap(), Box::new(FakeStreamer::default()));
    assert_eq!(get_json(&host, "/api/settings").await["language"], "en");
    let (status, saved) = send(&host, "PUT", "/api/settings", Some(json!({ "language": "ja" }))).await;
    assert_eq!((status, saved["language"].clone()), (StatusCode::OK, json!("ja")));
    let (status, err) = send(&host, "PUT", "/api/settings", Some(json!({ "language": "fr" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err["error"].as_str().unwrap().contains("language"), "{err}");
    // the settings form saves the other fields without the language
    let (status, _) = send(
        &host,
        "PUT",
        "/api/settings",
        Some(json!({ "downlinkTimeoutMs": 6000 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(get_json(&host, "/api/settings").await["language"], "ja");
}
