mod common;

use common::{bits_of, bytes, hex};
use sigfox::auth::{PUBLIC_KEY, uplink_auth, verify_uplink};
use sigfox::frame::{DeviceId, UplinkFields, decode_uplink_bits};

// Bitstreams built by sigfox-ep-lib (SIGFOX_EP_BITSTREAM_build_application_frame) for device 0A1B2C3D,
// counter 1234, downlink flag set and the public key: [payload, rank 1, rank 3]
const PUBLIC_VECTORS: [[&str; 3]; 15] = [
    ["empty", "aaaaa06b24d23d2c1b0a8cb34313", "aaaaa0342de6b2671dc82f9f93d7"],
    ["bit0", "aaaaa06ba4d23d2c1b0afa4d12bc", "aaaaa0348de6b2671dc844de5613"],
    ["bit1", "aaaaa06be4d23d2c1b0ad1d60ca2", "aaaaa034dde6b2671dc865a38f8a"],
    ["a5", "aaaaa08d24d23d2c1b0aa573b0dc70", "aaaaa3022de6b2671dc80c2f5ceb6c"],
    [
        "0102",
        "aaaaa35fa4d23d2c1b0a01025b17b66eb4f5",
        "aaaaa5a38de6b2671dc88142cdd25bf519c8",
    ],
    [
        "010203",
        "aaaaa35f64d23d2c1b0a010203ad562e52a4",
        "aaaaa5a37de6b2671dc88142834603a5c60d",
    ],
    [
        "01020304",
        "aaaaa35f24d23d2c1b0a01020304089d16e1",
        "aaaaa5a32de6b2671dc8814283c50aba5359",
    ],
    [
        "0102030405",
        "aaaaa611e4d23d2c1b0a0102030405e9b4e027fdb448",
        "aaaaa72cdde6b2671dc8814283c50493d9d82e02d95a",
    ],
    [
        "010203040506",
        "aaaaa611a4d23d2c1b0a010203040506960aaccb4ab4",
        "aaaaa72c8de6b2671dc8814283c50447338807f99819",
    ],
    [
        "01020304050607",
        "aaaaa61164d23d2c1b0a01020304050607009ebfe4e3",
        "aaaaa72c7de6b2671dc8814283c5044786c0b9101ddb",
    ],
    [
        "0102030405060708",
        "aaaaa61124d23d2c1b0a010203040506070808736be7",
        "aaaaa72c2de6b2671dc8814283c5044786ca0a6fb11e",
    ],
    [
        "010203040506070809",
        "aaaaa94ce4d23d2c1b0a01020304050607080960cff3ae92212f",
        "aaaaa997dde6b2671dc8814283c5044786ca0b38fc0f4536a964",
    ],
    [
        "0102030405060708090a",
        "aaaaa94ca4d23d2c1b0a0102030405060708090a984e18499535",
        "aaaaa9978de6b2671dc8814283c5044786ca0b483e5d9e5bf078",
    ],
    [
        "0102030405060708090a0b",
        "aaaaa94c64d23d2c1b0a0102030405060708090a0bd5a4ca6da3",
        "aaaaa9977de6b2671dc8814283c5044786ca0b488920cdf8f6cb",
    ],
    [
        "0102030405060708090a0b0c",
        "aaaaa94c24d23d2c1b0a0102030405060708090a0b0c25f6f4bd",
        "aaaaa9972de6b2671dc8814283c5044786ca0b4889cf2c8b4992",
    ],
];

fn key(h: &str) -> [u8; 16] {
    bytes(h).try_into().unwrap()
}

#[test]
fn the_public_key_is_the_one_defined_by_sigfox_ep_lib() {
    assert_eq!(hex(&PUBLIC_KEY), "00112233445566778899aabbccddeeff");
}

#[test]
fn public_key_frames_match_sigfox_ep_lib() {
    for [payload, rank1, rank3] in PUBLIC_VECTORS {
        for h in [rank1, rank3] {
            let f = decode_uplink_bits(&bits_of(h)).unwrap();
            assert!(f.crc_ok, "{payload}");
            assert_eq!(hex(&uplink_auth(&PUBLIC_KEY, &f.fields())), hex(&f.auth), "{payload}");
            assert!(verify_uplink(&PUBLIC_KEY, &f.fields(), &f.auth), "{payload}");
        }
    }
}

#[test]
fn a_frame_recorded_from_a_device_in_private_key_mode_does_not_verify_with_the_public_key() {
    // counter 1979 from device 00763DA3, payload 11, auth ab6d (see frame.rs)
    let f = UplinkFields {
        device_id: DeviceId(0x0076_3DA3),
        counter: 1979,
        payload: &[0x11],
        bit: None,
        downlink: false,
    };
    assert!(!verify_uplink(&PUBLIC_KEY, &f, &[0xab, 0x6d]));
}

#[test]
fn the_device_key_verifies_frames_built_with_it() {
    // sigfox-ep-lib with key 3f2a1099 0102030405060708090a0b0c, device 00763DA3, counter 1979, payload 11
    let f = decode_uplink_bits(&bits_of("aaaaa08d07bba33d7600111ebbcd78")).unwrap();
    assert!(verify_uplink(
        &key("3f2a10990102030405060708090a0b0c"),
        &f.fields(),
        &f.auth
    ));
    assert!(!verify_uplink(&PUBLIC_KEY, &f.fields(), &f.auth));
}
