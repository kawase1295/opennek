mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Request, StatusCode, Uri};
use axum::routing::any;
use common::{FakeStreamer, fixture, streamer_lines};
use http_body_util::BodyExt;
use opennek::engine::Engine;
use opennek::host::Host;
use opennek::radio::{RadioEvent, RadioMessage};
use opennek::store::{CallbackInput, Store};
use serde_json::{Value, json};
use sigfox::auth::{PUBLIC_KEY, uplink_auth};
use sigfox::frame::{DeviceId, UplinkFields};
use sigfox::receiver::SigfoxMessage;
use tower::ServiceExt;

#[derive(Clone, Debug)]
struct Hit {
    method: String,
    uri: String,
    content_type: String,
    body: String,
}

/// A user's server: records each request and answers with `reply`
async fn user_server(status: u16, reply: &'static str, delay: Duration) -> (String, Arc<Mutex<Vec<Hit>>>) {
    let hits = Arc::new(Mutex::new(vec![]));
    let seen = hits.clone();
    let app = Router::new().fallback(any(
        move |method: axum::http::Method, uri: Uri, headers: HeaderMap, body: Bytes| {
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(Hit {
                    method: method.to_string(),
                    uri: uri.to_string(),
                    content_type: headers
                        .get("content-type")
                        .map_or("", |v| v.to_str().unwrap())
                        .to_string(),
                    body: String::from_utf8_lossy(&body).into_owned(),
                });
                tokio::time::sleep(delay).await;
                (StatusCode::from_u16(status).unwrap(), reply)
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, hits)
}

async fn eventually_async<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    for _ in 0..1000 {
        if let Some(v) = f() {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

fn callback(url: &str, body: &str) -> CallbackInput {
    CallbackInput::parse(json!({ "name": "test", "url": url, "body": body })).unwrap()
}

async fn api(host: &Arc<Host>, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    let body = body.map_or(Body::empty(), |b| Body::from(b.to_string()));
    let res = opennek::server::router(host.clone())
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test(flavor = "multi_thread")]
async fn real_sigfox_uplinks_run_callbacks() {
    let (url, hits) = user_server(204, "", Duration::ZERO).await;
    let store = Store::open_in_memory().unwrap();
    let mut s = store.settings().unwrap();
    s.sigfox_port = "fake0".into();
    store.save_settings(&s).unwrap();
    store
        .create_callback(&callback(
            &format!("{url}/up?device={{device}}"),
            r#"{"device":"{device}","seq":{seqNumber},"data":"{data}","ack":{ack},"rssi":{rssi},"frames":{nbFrames},"station":"{station}"}"#,
        ))
        .unwrap();
    let raw = std::fs::read(fixture("rc3-00763DA3-live.rssi")).unwrap();
    let host = Host::new(store, Box::new(FakeStreamer::with_lines(streamer_lines(&raw))));
    host.radio.start();
    // the downlink status of the last message is settled after its callbacks have answered
    eventually_async("callbacks and settled statuses", || {
        let store = host.engine.store();
        let settled = store
            .list_messages(&Default::default())
            .unwrap()
            .iter()
            .all(|m| serde_json::to_value(m.fields.downlink_status).unwrap() != "pending");
        (hits.lock().unwrap().len() == 3 && settled).then_some(())
    })
    .await;
    host.radio.stop();

    let mut bodies: Vec<Value> = hits
        .lock()
        .unwrap()
        .iter()
        .map(|h| serde_json::from_str(&h.body).unwrap())
        .collect();
    bodies.sort_by_key(|b| b["seq"].as_i64());
    let summary: Vec<_> = bodies
        .iter()
        .map(|b| {
            (
                b["device"].clone(),
                b["seq"].clone(),
                b["data"].clone(),
                b["ack"].clone(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (json!("00763DA3"), json!(1982), json!("1234567890"), json!(false)),
            (json!("00763DA3"), json!(1983), json!("11"), json!(false)),
            (json!("00763DA3"), json!(1984), json!("11"), json!(true)),
        ]
    );
    assert_eq!(bodies[0]["station"], "0000");
    let hit = hits.lock().unwrap()[0].clone();
    assert_eq!(
        (hit.method.as_str(), hit.content_type.as_str()),
        ("POST", "application/json")
    );
    assert!(hit.uri.ends_with("/up?device=00763DA3"), "{}", hit.uri);
    let messages = host.engine.store().list_messages(&Default::default()).unwrap();
    assert_eq!(
        serde_json::to_value(messages[0].fields.downlink_status).unwrap(),
        "unsupported"
    );
    let logs = host.engine.store().list_logs(None, None, None).unwrap();
    assert_eq!(logs.len(), 3);
    assert!(logs.iter().all(|l| l.fields.ok && l.fields.status == Some(204)));
}

fn uplink(key: &[u8; 16], counter: u16, downlink: bool) -> RadioEvent {
    let f = UplinkFields {
        device_id: DeviceId(0x0A1B_2C3D),
        counter,
        payload: &[1, 2, 3],
        bit: None,
        downlink,
    };
    RadioEvent::Message(RadioMessage {
        message: SigfoxMessage {
            device_id: f.device_id,
            counter,
            payload: f.payload.to_vec(),
            bit: None,
            downlink,
            control: false,
            auth: uplink_auth(key, &f),
            time: 0.0,
            first_frame_end: None,
            rssi: -90.0,
            ranks: vec![0, 1, 2],
            frames: vec![],
            clock_error_ppm: 0,
        },
        received_at: opennek::radio::now_ms(),
        first_frame_end_at: None,
        uplink_freq_hz: None,
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn public_key_uplinks_run_callbacks_only_while_unknown_devices_are_accepted() {
    let (url, hits) = user_server(204, "", Duration::ZERO).await;
    for accept in [false, true] {
        let store = Store::open_in_memory().unwrap();
        let mut s = store.settings().unwrap();
        s.accept_unknown_devices = accept;
        store.save_settings(&s).unwrap();
        store.create_callback(&callback(&format!("{url}/"), "{}")).unwrap();
        let engine = Arc::new(Engine::new(store));
        engine.handle_radio(uplink(&PUBLIC_KEY, 1, false));
        if accept {
            eventually_async("a log", || {
                (!engine.store().list_logs(None, None, None).unwrap().is_empty()).then_some(())
            })
            .await;
        } else {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
    assert_eq!(hits.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_callback_that_does_not_answer_in_time_is_logged_as_a_timeout() {
    let (url, _) = user_server(200, "late", Duration::from_secs(3)).await;
    let store = Store::open_in_memory().unwrap();
    let mut s = store.settings().unwrap();
    s.downlink_timeout_ms = 500;
    store.save_settings(&s).unwrap();
    store.create_callback(&callback(&format!("{url}/"), "{}")).unwrap();
    let engine = Arc::new(Engine::new(store));
    let mut events = engine.subscribe();
    engine.handle_radio(uplink(&PUBLIC_KEY, 1, true));
    let log = eventually_async("a log", || engine.store().list_logs(None, None, None).unwrap().pop()).await;
    assert_eq!(
        (log.fields.ok, log.fields.status, log.fields.error.as_deref()),
        (false, None, Some("timeout (500 ms)"))
    );
    assert!(
        (500..2500).contains(&log.fields.duration_ms),
        "{}",
        log.fields.duration_ms
    );
    let names: Vec<&str> = std::iter::from_fn(|| events.try_recv().ok())
        .map(|e| e.name())
        .collect();
    assert!(names.contains(&"callbackLog"), "{names:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn callbacks_are_edited_and_tried_from_the_api() {
    let (url, hits) = user_server(
        200,
        r#"{"0A1B2C3D":{"downlinkData":"0102030405060708"}}"#,
        Duration::ZERO,
    )
    .await;
    let host = Host::new(Store::open_in_memory().unwrap(), Box::new(FakeStreamer::default()));
    let (status, created) = api(
        &host,
        "POST",
        "/api/callbacks",
        Some(json!({ "url": format!("{url}/dl?d={{device}}"), "downlink": true, "headers": { "X-Key": "{device}" } })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(
        created,
        json!({ "name": "", "enabled": true, "devices": "", "method": "POST", "url": format!("{url}/dl?d={{device}}"),
            "headers": { "X-Key": "{device}" }, "contentType": "application/json", "body": "", "downlink": true, "id": 1 })
    );
    let (status, err) = api(&host, "POST", "/api/callbacks", Some(json!({ "url": "not a url" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err["error"].as_str().unwrap().starts_with("url:"));
    let (status, err) = api(
        &host,
        "POST",
        "/api/callbacks",
        Some(json!({ "url": "http://x/", "method": "PATCH" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{err}");

    // tried with a made-up message when there is none
    let (status, tried) = api(&host, "POST", "/api/callbacks/1/test", None).await;
    assert_eq!(status, StatusCode::OK, "{tried}");
    assert_eq!(tried["message"]["deviceId"], "F0000001");
    assert_eq!(tried["result"]["ok"], true);
    assert_eq!(tried["result"]["status"], 200);
    // a test is not logged
    assert!(host.engine.store().list_logs(None, None, None).unwrap().is_empty());
    assert!(hits.lock().unwrap()[0].uri.ends_with("/dl?d=F0000001"));

    let (status, updated) = api(
        &host,
        "PUT",
        "/api/callbacks/1",
        Some(json!({ "url": "http://127.0.0.1:9/", "enabled": false })),
    )
    .await;
    assert_eq!(
        (status, updated["enabled"].clone(), updated["id"].clone()),
        (StatusCode::OK, json!(false), json!(1))
    );
    let (status, _) = api(&host, "PUT", "/api/callbacks/7", Some(json!({ "url": "http://x/" }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = api(&host, "POST", "/api/callbacks/7/test", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = api(&host, "DELETE", "/api/callbacks/1", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(api(&host, "GET", "/api/callbacks", None).await.1, json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_callback_response_can_carry_downlink_data() {
    let (url, _) = user_server(
        200,
        r#"{"0A1B2C3D":{"downlinkData":"0102030405060708"}}"#,
        Duration::ZERO,
    )
    .await;
    let store = Store::open_in_memory().unwrap();
    let mut cb = callback(&format!("{url}/"), "{}");
    cb.downlink = true;
    let cb = store.create_callback(&cb).unwrap();
    let message = opennek::callbacks::dummy_message(true);
    let message = opennek::store::Message {
        fields: opennek::store::NewMessage {
            device_id: "0A1B2C3D".into(),
            ..message.fields
        },
        ..message
    };
    let result = tokio::task::spawn_blocking(move || opennek::callbacks::run_callback(&cb, &message, "", 2000))
        .await
        .unwrap();
    assert_eq!(result.downlink.as_deref(), Some("0102030405060708"));
}

/// A user's server with awkward answers, by path; requests are recorded as "METHOD path body authorization"
async fn awkward_server() -> (String, Arc<Mutex<Vec<String>>>) {
    use axum::response::IntoResponse;
    let seen = Arc::new(Mutex::new(vec![]));
    let record = seen.clone();
    let app = Router::new().fallback(any(
        move |method: axum::http::Method, uri: Uri, headers: HeaderMap, body: Bytes| {
            let record = record.clone();
            async move {
                let auth = headers
                    .get("authorization")
                    .map_or("-", |v| v.to_str().unwrap())
                    .to_string();
                record
                    .lock()
                    .unwrap()
                    .push(format!("{method} {uri} {} {auth}", String::from_utf8_lossy(&body)));
                match uri.path() {
                    "/slow" => {
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                        StatusCode::NO_CONTENT.into_response()
                    }
                    "/307" => (StatusCode::TEMPORARY_REDIRECT, [("location", "/echo")]).into_response(),
                    "/303" => (StatusCode::SEE_OTHER, [("location", "/echo")]).into_response(),
                    "/bom" => b"\xef\xbb\xbf{\"downlinkData\":\"0011223344556677\"}"
                        .to_vec()
                        .into_response(),
                    "/latin1" => b"\xff\xfeA".to_vec().into_response(),
                    "/stall" => {
                        let chunks = tokio_stream::StreamExt::chain(
                            tokio_stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"{"))]),
                            tokio_stream::pending(),
                        );
                        Body::from_stream(chunks).into_response()
                    }
                    _ => StatusCode::OK.into_response(),
                }
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, seen)
}

fn try_callback(url: &str, extra: Value) -> opennek::callbacks::CallbackResult {
    let mut body = json!({ "url": url, "body": "b" });
    body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    let cb = opennek::store::Callback {
        input: CallbackInput::parse(body).unwrap(),
        id: 1,
    };
    opennek::callbacks::run_callback(&cb, &opennek::callbacks::dummy_message(false), "", 1000)
}

#[tokio::test(flavor = "multi_thread")]
async fn awkward_urls_and_answers_are_handled_like_fetch() {
    let (url, seen) = awkward_server().await;
    let run = |path: &str, extra: Value| {
        let url = format!("{url}{path}");
        std::thread::spawn(move || try_callback(&url, extra)).join().unwrap()
    };
    // the URL is normalized as fetch does
    let r = run("/a b/温", json!({}));
    assert_eq!((r.status, r.error.as_deref()), (Some(200), None));
    assert!(
        seen.lock()
            .unwrap()
            .last()
            .unwrap()
            .starts_with("POST /a%20b/%E6%B8%A9 ")
    );
    // 307 repeats the request, 303 turns it into a GET; the same origin keeps Authorization
    let r = run("/307", json!({ "headers": { "Authorization": "Bearer t" } }));
    assert_eq!(r.status, Some(200));
    assert_eq!(seen.lock().unwrap().last().unwrap(), "POST /echo b Bearer t");
    let r = run("/303", json!({}));
    assert_eq!(r.status, Some(200));
    assert_eq!(seen.lock().unwrap().last().unwrap(), "GET /echo  -");
    // text as fetch decodes it: lossy UTF-8 without a byte order mark
    let r = run("/latin1", json!({}));
    assert_eq!((r.ok, r.response.as_deref()), (true, Some("\u{fffd}\u{fffd}A")));
    let r = run("/bom", json!({ "downlink": true }));
    assert_eq!(r.downlink.as_deref(), Some("0011223344556677"));
    // a body that stops coming
    let r = run("/stall", json!({}));
    assert_eq!((r.status, r.error.as_deref()), (None, Some("timeout (1000 ms)")));
}

#[tokio::test(flavor = "multi_thread")]
async fn each_callback_is_logged_when_it_answers() {
    let (url, _) = awkward_server().await;
    let store = Store::open_in_memory().unwrap();
    store.create_callback(&callback(&format!("{url}/slow"), "{}")).unwrap();
    store.create_callback(&callback(&format!("{url}/fast"), "{}")).unwrap();
    let engine = Arc::new(Engine::new(store));
    engine.handle_radio(uplink(&PUBLIC_KEY, 1, false));
    // the fast one is logged while the slow one is still waiting
    let first = eventually_async("a log", || engine.store().list_logs(None, None, None).unwrap().pop()).await;
    assert_eq!(first.fields.callback_id, 2);
    assert_eq!(engine.store().list_logs(None, None, None).unwrap().len(), 1);
    eventually_async("both logs", || {
        (engine.store().list_logs(None, None, None).unwrap().len() == 2).then_some(())
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_callback_id_that_is_not_a_number_is_not_found() {
    let host = Host::new(Store::open_in_memory().unwrap(), Box::new(FakeStreamer::default()));
    assert_eq!(
        api(&host, "PUT", "/api/callbacks/abc", Some(json!({ "url": "http://x/" })))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api(&host, "POST", "/api/callbacks/abc/test", None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api(&host, "DELETE", "/api/callbacks/abc", None).await.0,
        StatusCode::NO_CONTENT
    );
}
