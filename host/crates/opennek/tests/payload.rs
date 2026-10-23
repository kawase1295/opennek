use opennek::payload::{decode_payload, parse_payload_config};
use serde_json::json;

fn bytes(h: &str) -> Vec<u8> {
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn sequential_fields_with_endianness_bits_and_bool() {
    // temp = -12.3 (int16 LE x10 = 0xff85), hum = 0x40, flags byte 0b1000_0001, 12-bit counter 0xabc
    let v = decode_payload(
        "temp::int:16:little-endian hum::uint:8 door:3:bool:7 low:3:bool:0 flags::uint:8 cnt::uint:12",
        &bytes("85ff4081abc0"),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        json!({ "temp": -123, "hum": 0x40, "door": true, "low": true, "flags": 0x81, "cnt": 0xabc }).to_string()
    );
}

#[test]
fn float_char_and_explicit_offsets() {
    let mut buf = 1.5f32.to_be_bytes().to_vec();
    buf.extend(b"OK\0\0");
    let v = decode_payload("v::float:32 s::char:2 again:0:uint:8", &buf).unwrap();
    assert_eq!(
        serde_json::Value::Object(v),
        json!({ "v": 1.5, "s": "OK", "again": 0x3f })
    );
}

#[test]
fn full_width_and_little_endian_fields() {
    let v = decode_payload(
        "a::uint:32 b::int:32:little-endian c::float:64:little-endian",
        &bytes("ffffffff feffffff 000000000000f83f".replace(' ', "").as_str()),
    )
    .unwrap();
    assert_eq!(
        serde_json::Value::Object(v),
        json!({ "a": 4_294_967_295u32, "b": -2, "c": 1.5 })
    );
}

#[test]
fn errors() {
    let err = |config: &str| parse_payload_config(config).unwrap_err();
    assert!(err("x::nope:8").contains("unknown type"));
    assert!(err("x::bool:1").contains("explicit byte offset"));
    assert!(err("1x::uint:8").contains("invalid field name"));
    assert!(err("x:-1:uint:8").contains("invalid offset"));
    assert!(err("x::uint:33").contains("bits must be 1..32"));
    assert!(err("x::uint:12:little-endian").contains("whole bytes"));
    assert!(err("x::uint:8:middle").contains("invalid endianness"));
    assert!(err("x::float:16").contains("32 or 64"));
    assert!(err("x::char:0").contains("char length"));
    assert!(
        decode_payload("x::uint:16", &[1])
            .unwrap_err()
            .contains("exceeds payload")
    );
    assert!(
        decode_payload("x::uint:4 y::char:1", &[1, 2])
            .unwrap_err()
            .contains("not byte aligned")
    );
    assert!(parse_payload_config("  ").unwrap().is_empty());
}

#[test]
fn whole_floats_are_json_integers_as_in_javascript() {
    let v = decode_payload(
        "a::float:32 b::float:32 c::float:64",
        &bytes("3f800000800000004340000000000000"),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_string(&v).unwrap(),
        r#"{"a":1,"b":0,"c":9007199254740992}"#
    );
}

#[test]
fn a_utf8_byte_order_mark_is_dropped_like_text_decoder_does() {
    let v = decode_payload("s:0:char:4", &bytes("efbbbf41")).unwrap();
    assert_eq!(v["s"], "A");
}

#[test]
fn huge_offsets_exceed_the_payload_instead_of_overflowing() {
    for config in [
        "a:2305843009213693952:uint:8",
        "a:2305843009213693951:uint:8",
        "a:18446744073709551615:char:2",
    ] {
        assert!(
            decode_payload(config, &[1, 2]).unwrap_err().contains("exceeds payload"),
            "{config}"
        );
    }
}
