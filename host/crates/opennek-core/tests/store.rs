use opennek_core::store::{Auth, DownlinkStatus, Language, MessageQuery, NewMessage, ReceiverPort, Settings, Store};
use serde_json::{Value, json};

fn uplink(device_id: &str, seq: u16) -> NewMessage {
    NewMessage {
        device_id: device_id.into(),
        time: 1_700_000_000_000,
        seq,
        data: "11".into(),
        ack: false,
        oob: false,
        rssi: -80,
        nb_frames: 3,
        auth: Auth::Unverified,
        station: "0000".into(),
        decoded: None,
        downlink: None,
        downlink_status: DownlinkStatus::None,
    }
}

#[test]
fn messages_are_listed_newest_first_with_the_api_shape() {
    let store = Store::open_in_memory().unwrap();
    store.insert_message(&uplink("00763DA3", 1)).unwrap();
    store.insert_message(&uplink("0A1B2C3D", 2)).unwrap();
    let m = store.insert_message(&uplink("00763DA3", 3)).unwrap();
    assert_eq!(
        serde_json::to_value(&m).unwrap(),
        json!({
            "id": 3, "deviceId": "00763DA3", "time": 1_700_000_000_000i64, "seq": 3, "data": "11", "ack": false,
            "oob": false, "rssi": -80, "nbFrames": 3, "auth": "unverified", "station": "0000", "decoded": null,
            "downlink": null, "downlinkStatus": "none"
        })
    );
    let seqs = |q: MessageQuery| {
        store
            .list_messages(&q)
            .unwrap()
            .iter()
            .map(|m| m.fields.seq)
            .collect::<Vec<_>>()
    };
    assert_eq!(seqs(MessageQuery::default()), [3, 2, 1]);
    assert_eq!(
        seqs(MessageQuery {
            device_id: Some("00763DA3".into()),
            ..Default::default()
        }),
        [3, 1]
    );
    assert_eq!(
        seqs(MessageQuery {
            before: Some(3),
            limit: Some(1),
            ..Default::default()
        }),
        [2]
    );
}

#[test]
fn devices_seen_on_air_are_created_and_touched() {
    let store = Store::open_in_memory().unwrap();
    store.ensure_device("00763DA3", 1000).unwrap();
    store.ensure_device("00763DA3", 2000).unwrap();
    store.touch_device("00763DA3", 1979, 3000).unwrap();
    let devices = serde_json::to_value(store.list_devices().unwrap()).unwrap();
    assert_eq!(
        devices,
        json!([{
            "id": "00763DA3", "name": "", "key": "", "downlinkMode": "none", "downlinkData": "", "payloadConfig": "",
            "createdAt": 1000, "lastSeen": 3000, "lastSeq": 1979
        }])
    );
}

#[test]
fn settings_default_merge_and_validate_like_the_typescript_host() {
    let store = Store::open_in_memory().unwrap();
    let s = store.settings().unwrap();
    assert_eq!(s, Settings::default());
    assert_eq!(
        serde_json::to_value(&s).unwrap(),
        json!({
            "downlinkTimeoutMs": 5000, "acceptUnknownDevices": true, "sigfoxPort": "", "sigfoxFreq": 923_200_000,
            "sigfoxDownlink": false, "language": "en"
        })
    );
    let changed = s.merged(json!({ "sigfoxPort": "/dev/ttyACM0", "unknown": 1 })).unwrap();
    store.save_settings(&changed).unwrap();
    assert_eq!(
        store.settings().unwrap().sigfox_port,
        ReceiverPort::Port("/dev/ttyACM0".into())
    );
    assert!(
        s.merged(json!({ "sigfoxFreq": 100 }))
            .unwrap_err()
            .contains("sigfoxFreq")
    );
    assert!(s.merged(json!({ "downlinkTimeoutMs": "x" })).is_err());
}

#[test]
fn the_receiver_port_is_never_set_none_or_a_port() {
    let s = Settings::default();
    assert_eq!(s.sigfox_port, ReceiverPort::Unset);
    for (json, port) in [
        (json!(""), ReceiverPort::Unset),
        (Value::Null, ReceiverPort::Off),
        (json!("COM4"), ReceiverPort::Port("COM4".into())),
    ] {
        let merged = s.merged(json!({ "sigfoxPort": json })).unwrap();
        assert_eq!(merged.sigfox_port, port);
        assert_eq!(serde_json::to_value(&merged).unwrap()["sigfoxPort"], json);
    }
    assert!(s.merged(json!({ "sigfoxPort": 1 })).is_err());
}

#[test]
fn an_empty_port_saved_by_an_older_version_is_read_as_never_set() {
    // Older versions saved "" for "none" and whenever another setting was saved, so it is no choice
    let store = Store::open_in_memory().unwrap();
    store
        .save_settings(&Settings::default().merged(json!({ "language": "ja" })).unwrap())
        .unwrap();
    assert_eq!(store.settings().unwrap().sigfox_port, ReceiverPort::Unset);
    let off = store.settings().unwrap().merged(json!({ "sigfoxPort": null })).unwrap();
    store.save_settings(&off).unwrap();
    assert_eq!(store.settings().unwrap().sigfox_port, ReceiverPort::Off);
}

#[test]
fn settings_keep_the_ui_language() {
    let store = Store::open_in_memory().unwrap();
    let s = store.settings().unwrap();
    assert_eq!(s.language, Language::En);
    let ja = s.merged(json!({ "language": "ja" })).unwrap();
    assert_eq!(ja.language, Language::Ja);
    store.save_settings(&ja).unwrap();
    assert_eq!(
        serde_json::to_value(store.settings().unwrap()).unwrap()["language"],
        "ja"
    );
    assert_eq!(ja.merged(json!({ "language": "en" })).unwrap().language, Language::En);
    for bad in [json!("fr"), json!(""), json!("JA"), json!(1), Value::Null] {
        let e = s.merged(json!({ "language": bad })).unwrap_err();
        assert!(e.starts_with("language: "), "{bad}: {e}");
    }
}

// The TypeScript host's schema, with the columns of a database from before the ESP-NOW network was removed (#19)
const OLD_SCHEMA: &str = "
CREATE TABLE devices (
  id TEXT PRIMARY KEY, name TEXT NOT NULL DEFAULT '', key TEXT NOT NULL DEFAULT '',
  downlink_mode TEXT NOT NULL DEFAULT 'none', downlink_data TEXT NOT NULL DEFAULT '',
  payload_config TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL, last_seen INTEGER, last_seq INTEGER
);
CREATE TABLE messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT, device_id TEXT NOT NULL, time INTEGER NOT NULL, seq INTEGER NOT NULL,
  data TEXT NOT NULL, ack INTEGER NOT NULL, oob INTEGER NOT NULL, rssi INTEGER NOT NULL, nb_frames INTEGER NOT NULL,
  auth TEXT NOT NULL, station TEXT NOT NULL, decoded TEXT, downlink TEXT, downlink_status TEXT NOT NULL DEFAULT 'none',
  radio TEXT NOT NULL DEFAULT 'sigfox', src TEXT
);
CREATE TABLE callbacks (id INTEGER PRIMARY KEY AUTOINCREMENT, data TEXT NOT NULL);
CREATE TABLE callback_logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT, callback_id INTEGER NOT NULL, message_id INTEGER NOT NULL, time INTEGER NOT NULL,
  status INTEGER, ok INTEGER NOT NULL, error TEXT, duration_ms INTEGER NOT NULL, response TEXT
);
CREATE TABLE settings (id INTEGER PRIMARY KEY CHECK (id = 1), data TEXT NOT NULL);
INSERT INTO messages (device_id, time, seq, data, ack, oob, rssi, nb_frames, auth, station, decoded, downlink,
  downlink_status, radio)
VALUES ('00763DA3', 1, 7, '0908', 0, 1, -90, 2, 'public', '0000', '{\"type\":\"keepAlive\"}', NULL, 'none', 'sigfox'),
       ('E5000001', 2, 8, '01', 1, 0, -50, 1, 'ok', '0000', NULL, 'aabb', 'sent', 'espnow');
INSERT INTO callback_logs (callback_id, message_id, time, status, ok, error, duration_ms, response)
VALUES (1, 1, 5, 200, 1, NULL, 12, 'ok'), (1, 2, 6, 500, 0, 'boom', 3, NULL);
INSERT INTO callbacks (data) VALUES ('{\"name\":\"n\",\"url\":\"http://x/\"}');
INSERT INTO settings (id, data) VALUES (1, '{\"sigfoxPort\":\"COM4\",\"sigfoxDownlink\":true}');
";

#[test]
fn a_database_of_the_typescript_host_opens_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opennek.db");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(OLD_SCHEMA)
        .unwrap();

    let store = Store::open(&path).unwrap();
    // only the Sigfox message survives the ESP-NOW cleanup
    let messages = serde_json::to_value(store.list_messages(&MessageQuery::default()).unwrap()).unwrap();
    assert_eq!(messages[0]["deviceId"], "00763DA3");
    assert_eq!(messages[0]["decoded"], json!({ "type": "keepAlive" }));
    assert_eq!(messages[0]["oob"], true);
    assert_eq!(messages.as_array().unwrap().len(), 1);
    assert_eq!(store.list_logs(None, None, None).unwrap().len(), 1);
    assert_eq!(store.list_callbacks().unwrap()[0].id, 1);
    let s = store.settings().unwrap();
    assert_eq!(
        (s.sigfox_port, s.sigfox_downlink, s.sigfox_freq),
        (ReceiverPort::Port("COM4".into()), true, 923_200_000)
    );
    // saved before the language was a setting
    assert_eq!(s.language, Language::En);
    drop(store);
    // and opening it again is a no-op
    assert_eq!(
        Store::open(&path)
            .unwrap()
            .list_messages(&MessageQuery::default())
            .unwrap()
            .len(),
        1
    );
}
