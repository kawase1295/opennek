//! Callbacks with the types, channels and fields of the Sigfox Backend (its device type's "New callback")

use opennek_core::store::{CallbackInput, CallbackSubtype, CallbackType, Channel, HttpMethod, Store};
use serde_json::{Value, json};

fn parse(body: Value) -> Result<CallbackInput, String> {
    CallbackInput::parse(body)
}

#[test]
fn a_new_callback_is_a_data_uplink_to_a_url_as_in_the_backend() {
    let c = parse(json!({ "url": "https://example.com/up" })).unwrap();
    assert_eq!(
        serde_json::to_value(&c).unwrap(),
        json!({
            "name": "", "callbackType": "DATA", "callbackSubtype": "UPLINK", "channel": "URL", "enabled": true,
            "sendDuplicate": false, "payloadConfig": "", "url": "https://example.com/up", "httpMethod": "POST",
            "sendSni": true, "headers": {}, "contentType": "application/json", "bodyTemplate": "", "linePattern": "",
            "recipient": "", "subject": "", "message": "", "downlink": false, "devices": ""
        })
    );
}

#[test]
fn every_type_and_subtype_of_the_backend_can_be_saved() {
    for (t, s) in [
        ("DATA", Some("UPLINK")),
        ("DATA", Some("BIDIR")),
        ("SERVICE", Some("STATUS")),
        ("SERVICE", Some("ACKNOWLEDGE")),
        ("SERVICE", Some("REPEATER")),
        ("SERVICE", Some("DATA_ADVANCED")),
        ("ERROR", None),
    ] {
        let c = parse(json!({ "callbackType": t, "callbackSubtype": s, "url": "http://x/" }))
            .unwrap_or_else(|e| panic!("{t} {s:?}: {e}"));
        assert_eq!(serde_json::to_value(c.callback_type).unwrap(), t);
        assert_eq!(serde_json::to_value(c.callback_subtype).unwrap(), json!(s));
    }
    // a subtype must belong to its type
    for (t, s) in [
        ("DATA", json!("STATUS")),
        ("SERVICE", json!("UPLINK")),
        ("ERROR", json!("UPLINK")),
        ("SERVICE", Value::Null),
    ] {
        let e = parse(json!({ "callbackType": t, "callbackSubtype": s, "url": "http://x/" })).unwrap_err();
        assert!(e.starts_with("callbackSubtype: "), "{t} {s}: {e}");
    }
    assert!(
        parse(json!({ "callbackType": "EVENT", "url": "http://x/" }))
            .unwrap_err()
            .starts_with("callbackType: ")
    );
}

#[test]
fn each_channel_needs_its_own_fields() {
    let batch =
        parse(json!({ "channel": "BATCH_URL", "url": "http://x/", "linePattern": "{device};{data};" })).unwrap();
    assert_eq!(batch.channel, Channel::BatchUrl);
    let mail = parse(json!({ "channel": "EMAIL", "recipient": "ops@example.com", "subject": "{device}" })).unwrap();
    assert_eq!(mail.channel, Channel::Email);
    for (body, field) in [
        (json!({ "channel": "URL" }), "url"),
        (json!({ "channel": "URL", "url": "not a url" }), "url"),
        (json!({ "channel": "BATCH_URL" }), "url"),
        (json!({ "channel": "EMAIL" }), "recipient"),
        (json!({ "channel": "EMAIL", "recipient": "nobody" }), "recipient"),
        (json!({ "channel": "SMS", "url": "http://x/" }), "channel"),
        (json!({ "url": "http://x/", "httpMethod": "PATCH" }), "httpMethod"),
        (json!({ "url": "http://x/", "headers": { "X-N": 1 } }), "headers.X-N"),
        (
            json!({ "url": "http://x/", "payloadConfig": "temp::nope:8" }),
            "payloadConfig",
        ),
    ] {
        let e = parse(body.clone()).unwrap_err();
        assert!(e.starts_with(&format!("{field}: ")), "{body}: {e}");
    }
}

#[test]
fn only_a_data_bidir_callback_is_chosen_for_downlink() {
    let c = parse(json!({ "callbackSubtype": "BIDIR", "url": "http://x/", "downlink": true })).unwrap();
    assert!(c.downlink);
    let e = parse(json!({ "callbackSubtype": "UPLINK", "url": "http://x/", "downlink": true })).unwrap_err();
    assert!(e.starts_with("downlink: "), "{e}");
}

#[test]
fn opennek_sends_only_data_callbacks_to_a_url() {
    let sent = |body: Value| parse(body).unwrap().is_sent();
    assert!(sent(json!({ "url": "http://x/" })));
    assert!(sent(json!({ "callbackSubtype": "BIDIR", "url": "http://x/" })));
    assert!(!sent(
        json!({ "callbackType": "SERVICE", "callbackSubtype": "STATUS", "url": "http://x/" })
    ));
    assert!(!sent(json!({ "callbackType": "ERROR", "url": "http://x/" })));
    assert!(!sent(json!({ "channel": "BATCH_URL", "url": "http://x/" })));
    assert!(!sent(json!({ "channel": "EMAIL", "recipient": "a@b.c" })));
}

#[test]
fn callbacks_saved_by_older_versions_read_as_data_callbacks_to_a_url() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opennek.db");
    drop(Store::open(&path).unwrap());
    let db = rusqlite::Connection::open(&path).unwrap();
    // what older versions stored: method, body, and downlink for "take the downlink from the response"
    for old in [
        json!({ "name": "n", "enabled": true, "devices": "", "method": "PUT", "url": "http://x/", "headers": {},
            "contentType": "text/plain", "body": "{data}", "downlink": false }),
        json!({ "name": "d", "enabled": true, "devices": "00763DA3", "method": "POST", "url": "http://y/",
            "headers": { "A": "b" }, "contentType": "application/json", "body": "{}", "downlink": true }),
    ] {
        db.execute("INSERT INTO callbacks (data) VALUES (?)", [old.to_string()])
            .unwrap();
    }
    drop(db);
    let cbs = Store::open(&path).unwrap().list_callbacks().unwrap();
    let up = &cbs[0].input;
    assert_eq!(
        (up.callback_type, up.callback_subtype, up.channel, up.http_method),
        (
            CallbackType::Data,
            Some(CallbackSubtype::Uplink),
            Channel::Url,
            HttpMethod::Put
        )
    );
    assert_eq!(
        (up.body_template.as_str(), up.content_type.as_str()),
        ("{data}", "text/plain")
    );
    // they sent SNI, as every TLS connection of ureq does
    assert!(up.send_sni);
    let dl = &cbs[1].input;
    assert_eq!(
        (dl.callback_subtype, dl.downlink, dl.devices.as_str()),
        (Some(CallbackSubtype::Bidir), true, "00763DA3")
    );
}
