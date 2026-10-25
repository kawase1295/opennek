use opennek_core::downlink::{DownlinkContext, parse_downlink_template, render_downlink};
use opennek_core::store::DeviceInput;
use serde_json::json;

const CTX: DownlinkContext<'static> = DownlinkContext {
    time: 1_700_000_000_123,
    station: "0000",
    rssi: -100,
};

fn render(template: &str, ctx: DownlinkContext) -> String {
    render_downlink(template, &ctx).unwrap()
}

#[test]
fn plain_8_byte_hex_renders_as_itself() {
    assert_eq!(render("0102030405060708", CTX), "0102030405060708");
}

#[test]
fn time_is_the_uplink_unix_time_in_seconds_big_endian() {
    // 1700000000 = 0x6553f100
    assert_eq!(render("{time}01020304", CTX), "6553f10001020304");
}

#[test]
fn rssi_is_a_signed_16_bit_big_endian_value() {
    assert_eq!(render("{time}0102{rssi}", CTX), "6553f1000102ff9c");
    assert_eq!(
        render("{time}0102{rssi}", DownlinkContext { rssi: 5, ..CTX }),
        "6553f10001020005"
    );
}

#[test]
fn rssi_saturates_to_the_int16_range() {
    assert_eq!(
        render("000000000000{rssi}", DownlinkContext { rssi: -40000, ..CTX }),
        "0000000000008000"
    );
    assert_eq!(
        render("000000000000{rssi}", DownlinkContext { rssi: 40000, ..CTX }),
        "0000000000007fff"
    );
}

#[test]
fn tapid_is_the_station_parsed_as_hex_and_left_padded_to_4_bytes() {
    assert_eq!(render("{tapid}00000000", CTX), "0000000000000000");
    assert_eq!(
        render(
            "{tapid}00000000",
            DownlinkContext {
                station: "AABBCC",
                ..CTX
            }
        ),
        "00aabbcc00000000"
    );
    assert_eq!(
        render(
            "{tapid}00000000",
            DownlinkContext {
                station: "11AABBCC",
                ..CTX
            }
        ),
        "11aabbcc00000000"
    );
    // not hex: zero
    assert_eq!(
        render(
            "{tapid}00000000",
            DownlinkContext {
                station: "sigfox",
                ..CTX
            }
        ),
        "0000000000000000"
    );
}

#[test]
fn templates_are_rejected_unless_they_render_to_exactly_8_bytes() {
    for t in ["{time}0102", "{time}{time}00", ""] {
        assert!(parse_downlink_template(t).unwrap_err().contains("8 bytes"), "{t}");
    }
}

#[test]
fn unknown_variables_odd_hex_digits_and_non_hex_characters_are_rejected() {
    let err = |t: &str| parse_downlink_template(t).unwrap_err();
    assert!(err("{foo}01020304").contains("{foo}"));
    assert!(err("{TIME}01020304").contains("{TIME}"));
    assert!(err("{time}010{rssi}0").contains("even number"));
    assert!(err("{time}0102zz{rssi}").contains("not hex"));
    assert!(err("{time01020304").contains("not hex"));
}

fn device(extra: serde_json::Value) -> Result<DeviceInput, String> {
    let mut body = json!({ "downlinkMode": "static" });
    body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    DeviceInput::parse("0a1b2c3d", body)
}

#[test]
fn device_input_accepts_templates_and_lower_cases_the_hex_parts() {
    assert_eq!(
        device(json!({ "downlinkData": "{time}AABB{rssi}" }))
            .unwrap()
            .downlink_data,
        "{time}aabb{rssi}"
    );
    assert_eq!(
        device(json!({ "downlinkData": "AABBCCDDEEFF0011" }))
            .unwrap()
            .downlink_data,
        "aabbccddeeff0011"
    );
    assert_eq!(device(json!({ "downlinkData": "" })).unwrap().downlink_data, "");
}

#[test]
fn device_input_rejects_invalid_templates_with_the_parser_message() {
    assert!(
        device(json!({ "downlinkData": "{foo}01020304" }))
            .unwrap_err()
            .contains("{foo}")
    );
    assert!(device(json!({ "downlinkData": "0102" })).is_err());
}

#[test]
fn device_input_checks_and_normalizes_every_field() {
    let d = device(json!({ "key": "3F2A10990102030405060708090A0B0C", "name": "kit", "payloadConfig": "t::int:16" }))
        .unwrap();
    assert_eq!(
        (d.id.as_str(), d.key.as_str(), d.name.as_str()),
        ("0A1B2C3D", "3f2a10990102030405060708090a0b0c", "kit")
    );
    assert!(DeviceInput::parse("0A1B2C3", json!({})).unwrap_err().starts_with("id:"));
    assert!(device(json!({ "key": "abcd" })).unwrap_err().starts_with("key:"));
    assert!(
        device(json!({ "downlinkMode": "sometimes" }))
            .unwrap_err()
            .starts_with("downlinkMode:")
    );
    assert!(
        device(json!({ "payloadConfig": "x::nope:8" }))
            .unwrap_err()
            .contains("unknown type")
    );
    assert!(
        device(json!({ "name": "n".repeat(65) }))
            .unwrap_err()
            .starts_with("name:")
    );
    // defaults
    let d = DeviceInput::parse("0A1B2C3D", json!({})).unwrap();
    assert_eq!(
        (d.downlink_mode.as_str(), d.key.as_str(), d.payload_config.as_str()),
        ("none", "", "")
    );
}

#[test]
fn a_station_with_a_sign_is_not_hex() {
    assert_eq!(
        render("{tapid}00000000", DownlinkContext { station: "+1", ..CTX }),
        "0000000000000000"
    );
}
