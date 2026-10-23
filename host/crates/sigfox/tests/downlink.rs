mod common;

use common::{bytes, hex};
use sigfox::auth::PUBLIC_KEY;
use sigfox::downlink::{DownlinkWindow, crc8, downlink_frequency, downlink_window, encode_downlink, whiten};
use sigfox::frame::DeviceId;

const PRIVATE_KEY: [u8; 16] = [0x3f, 0x2a, 0x10, 0x99, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

// DL-PHY contents accepted by sigfox-ep-lib's SIGFOX_EP_BITSTREAM_decode_downlink_frame (tools/sigfox-ep-oracle),
// which returned the payload for each: [device ID, counter, key, payload, frame]
const VECTORS: [(u32, u16, &[u8; 16], &str, &str); 5] = [
    (
        0x0A1B_2C3D,
        1234,
        &PUBLIC_KEY,
        "0102030405060708",
        "d1232ec88a6a34718ab5d1508837e4",
    ),
    (
        0x0A1B_2C3D,
        0,
        &PUBLIC_KEY,
        "deadbeefcafebabe",
        "fc5e5202028572a720c3ce39884f08",
    ),
    (
        0x0076_3DA3,
        1984,
        &PRIVATE_KEY,
        "0000000000000000",
        "ba00921dd6b681f678cb1d452a7d68",
    ),
    (
        0xFFFF_FFFF,
        4095,
        &PUBLIC_KEY,
        "ffffffffffffffff",
        "061dbfd49b8eaf26c1b862764dc409",
    ),
    (
        0x0000_0200,
        1,
        &PUBLIC_KEY,
        "1122334455667788",
        "4d22530dcd0aff0cbf5b030f7cdce8",
    ),
];

fn payload(h: &str) -> [u8; 8] {
    bytes(h).try_into().unwrap()
}

#[test]
fn downlink_frames_are_the_ones_sigfox_ep_lib_accepts() {
    for (id, counter, key, data, frame) in VECTORS {
        assert_eq!(
            hex(&encode_downlink(DeviceId(id), counter, &payload(data), key)),
            frame,
            "{id:08X} {counter}"
        );
    }
}

#[test]
fn the_frame_depends_on_the_key_and_the_counter_of_the_uplink() {
    let id = DeviceId(0x0A1B_2C3D);
    let data = payload("0102030405060708");
    // sigfox-ep-lib rejects the first vector with the private key or with counter 1235
    assert_ne!(hex(&encode_downlink(id, 1234, &data, &PRIVATE_KEY)), VECTORS[0].4);
    assert_ne!(hex(&encode_downlink(id, 1235, &data, &PUBLIC_KEY)), VECTORS[0].4);
}

#[test]
fn whitening_is_its_own_inverse() {
    let phy: [u8; 15] = bytes("00112233445566778899aabbccddee").try_into().unwrap();
    let id = DeviceId(0x0A1B_2C3D);
    assert_eq!(whiten(&whiten(&phy, id, 7), id, 7), phy);
}

#[test]
fn crc8_uses_polynomial_0x2f_with_a_zero_start_value() {
    assert_eq!(crc8(&[0; 10]), 0);
    assert_eq!(crc8(&[1]), 0x2f);
}

#[test]
fn rc3_downlinks_go_1_mhz_below_the_uplink() {
    assert_eq!(downlink_frequency(923_200_488), 922_200_488);
}

#[test]
fn the_downlink_is_sent_inside_the_receive_window() {
    // T_W 19 s after frame 1, 2 s into the window; the window lasts T_RX 33.5 s, 1 s margin for the frame
    assert_eq!(
        downlink_window(1_000),
        DownlinkWindow {
            at: 22_000,
            deadline: 52_500
        }
    );
}
