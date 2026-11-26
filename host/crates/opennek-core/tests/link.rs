mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::FakeStreamer;
use http_body_util::BodyExt;
use opennek_core::host::Host;
use opennek_core::link::{AuthCounts, LinkStats, RssiPoint, RssiSummary, missed_uplinks};
use opennek_core::store::{Auth, DownlinkStatus, NewMessage, Store};
use serde_json::{Value, json};
use tower::ServiceExt;

#[test]
fn no_uplinks_or_one_miss_nothing() {
    assert_eq!(missed_uplinks([]), 0);
    assert_eq!(missed_uplinks([7]), 0);
}

#[test]
fn consecutive_uplinks_miss_nothing() {
    assert_eq!(missed_uplinks([10, 11, 12, 13]), 0);
}

#[test]
fn each_seq_skipped_is_an_uplink_missed() {
    // 11, then 13 and 14, then 16 to 19
    assert_eq!(missed_uplinks([10, 12, 15, 20]), 1 + 2 + 4);
}

#[test]
fn the_counter_wraps_after_4095() {
    assert_eq!(missed_uplinks([4094, 4095, 0, 1]), 0);
    // 4095 and 0
    assert_eq!(missed_uplinks([4094, 1]), 2);
}

#[test]
fn a_restarted_counter_is_not_counted_as_loss() {
    // from 500 to 0 would be 3595 uplinks
    assert_eq!(missed_uplinks([499, 500, 0, 1, 3]), 1);
}

#[test]
fn a_jump_too_large_to_be_loss_is_not_counted() {
    assert_eq!(missed_uplinks([10, 2000, 2001]), 0);
    // 256 ahead is still loss
    assert_eq!(missed_uplinks([10, 266]), 255);
}

#[test]
fn a_late_uplink_is_not_counted() {
    assert_eq!(missed_uplinks([10, 11, 12, 11, 13]), 0);
    assert_eq!(missed_uplinks([4095, 0, 4095, 1]), 0);
}

#[test]
fn a_repeated_uplink_is_not_counted() {
    assert_eq!(missed_uplinks([10, 10, 11, 11, 12]), 0);
}

fn uplink(device_id: &str, seq: u16, time: i64, rssi: i32, nb_frames: u32, auth: Auth) -> NewMessage {
    NewMessage {
        device_id: device_id.into(),
        time,
        seq,
        data: "11".into(),
        ack: false,
        oob: false,
        rssi,
        nb_frames,
        auth,
        station: "0000".into(),
        decoded: None,
        downlink: None,
        downlink_status: DownlinkStatus::None,
    }
}

// 00763DA3: seq 10, 11, 14 and 15 (12 and 13 missed); 0A1B2C3D: one message in between
fn store_with_uplinks() -> Store {
    let store = Store::open_in_memory().unwrap();
    for m in [
        uplink("00763DA3", 10, 1000, -90, 3, Auth::Ok),
        uplink("0A1B2C3D", 500, 1500, -60, 1, Auth::Bad),
        uplink("00763DA3", 11, 2000, -100, 2, Auth::Bad),
        uplink("00763DA3", 14, 3000, -110, 1, Auth::Public),
        uplink("00763DA3", 15, 4000, -81, 3, Auth::Unverified),
    ] {
        store.insert_message(&m).unwrap();
    }
    store
}

#[test]
fn a_device_without_messages_has_no_link_stats() {
    let store = store_with_uplinks();
    assert_eq!(store.link_stats("FFFFFFFF").unwrap(), None);
    store.clear_messages(Some("00763DA3")).unwrap();
    assert_eq!(store.link_stats("00763DA3").unwrap(), None);
}

#[test]
fn link_stats_sum_up_the_messages_of_one_device() {
    let stats = store_with_uplinks().link_stats("00763DA3").unwrap();
    assert_eq!(
        stats,
        Some(LinkStats {
            messages: 4,
            first: 1000,
            last: 4000,
            rssi: RssiSummary {
                min: -110,
                avg: (-90.0 - 100.0 - 110.0 - 81.0) / 4.0,
                max: -81
            },
            frames: [1, 1, 2],
            auth: AuthCounts {
                ok: 1,
                bad: 1,
                public: 1,
                unverified: 1
            },
            missed: 2,
            points: vec![
                RssiPoint { time: 1000, rssi: -90 },
                RssiPoint { time: 2000, rssi: -100 },
                RssiPoint { time: 3000, rssi: -110 },
                RssiPoint { time: 4000, rssi: -81 },
            ],
        })
    );
}

async fn get_json(host: &Arc<Host>, path: &str) -> Value {
    let res = opennek_core::server::router(host.clone())
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "{path}");
    serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_ui_gets_the_link_stats_of_a_device() {
    let host = Host::new(store_with_uplinks(), Box::new(FakeStreamer::default()));
    assert_eq!(
        get_json(&host, "/api/devices/00763da3/stats").await,
        json!({
            "messages": 4, "first": 1000, "last": 4000,
            "rssi": { "min": -110, "avg": -95.25, "max": -81 },
            "frames": [1, 1, 2],
            "auth": { "ok": 1, "bad": 1, "public": 1, "unverified": 1 },
            "missed": 2,
            "points": [
                { "time": 1000, "rssi": -90 }, { "time": 2000, "rssi": -100 },
                { "time": 3000, "rssi": -110 }, { "time": 4000, "rssi": -81 }
            ]
        })
    );
    assert_eq!(get_json(&host, "/api/devices/FFFFFFFF/stats").await, Value::Null);
}
