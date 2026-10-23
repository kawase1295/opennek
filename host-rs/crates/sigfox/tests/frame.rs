mod common;

use common::{bytes, hex};
use sigfox::auth::{PUBLIC_KEY, uplink_auth};
use sigfox::frame::{
    ControlKind, ControlMessage, DeviceId, UplinkFields, convolve, crc16, decode_control, decode_uplink_bits,
    deconvolve, encode_uplink,
};

const DEVICE: DeviceId = DeviceId(0x0076_3DA3);

fn fields(counter: u16, payload: &[u8]) -> UplinkFields<'_> {
    UplinkFields {
        device_id: DEVICE,
        counter,
        payload,
        bit: None,
        downlink: false,
    }
}

// The receiver never sees the first preamble bit
fn first_bit_lost(bits: &[bool]) -> Vec<bool> {
    std::iter::once(true).chain(bits[1..].iter().copied()).collect()
}

#[test]
fn device_ids_print_as_at_id_does() {
    assert_eq!(DEVICE.to_string(), "00763DA3");
    assert_eq!("00763da3".parse::<DeviceId>(), Ok(DEVICE));
    assert!("123456789".parse::<DeviceId>().is_err());
    assert!("xyz".parse::<DeviceId>().is_err());
}

#[test]
fn convolution_is_invertible_for_every_rank() {
    let bits: Vec<bool> = "1011001110001011010011110000101".chars().map(|c| c == '1').collect();
    for rank in 0..3 {
        assert_eq!(deconvolve(&convolve(&bits, rank), rank), bits);
    }
}

#[test]
fn round_trip_for_every_payload_size_and_repetition() {
    for n in 0..=12u16 {
        for rank in 0..3 {
            let payload: Vec<u8> = (0..n).map(|i| ((i * 37 + n) & 0xff) as u8).collect();
            let li = if n >= 2 { 3 - (n - 1) % 4 } else { 0 };
            let auth: Vec<u8> = (0..li + 2).map(|i| 0xa0 + i as u8).collect();
            let f = UplinkFields {
                downlink: n % 2 == 1,
                ..fields(1979 + n, &payload)
            };
            let frame = decode_uplink_bits(&first_bit_lost(&encode_uplink(&f, rank, &auth, false))).unwrap();
            assert_eq!(
                (
                    frame.rank,
                    frame.device_id,
                    frame.counter,
                    frame.downlink,
                    frame.crc_ok,
                    frame.repaired
                ),
                (rank, DEVICE, 1979 + n, n % 2 == 1, true, None)
            );
            assert_eq!(hex(&frame.payload), hex(&payload));
            assert_eq!(hex(&frame.auth), hex(&auth));
        }
    }
}

#[test]
fn single_bit_messages() {
    let f = UplinkFields {
        device_id: DeviceId(1),
        counter: 5,
        payload: &[],
        bit: Some(true),
        downlink: false,
    };
    let frame = decode_uplink_bits(&encode_uplink(&f, 0, &[0, 0], false)).unwrap();
    assert_eq!((frame.bit, frame.crc_ok), (Some(true), true));
}

#[test]
fn a_slipped_bit_is_repaired_using_the_crc() {
    let bits = encode_uplink(&fields(1979, &[0x11]), 0, &[0xab, 0x6d], false);
    let slipped: Vec<bool> = [&bits[..70], &bits[71..]].concat();
    let frame = decode_uplink_bits(&slipped).unwrap();
    assert!(frame.crc_ok);
    assert!(frame.repaired.unwrap().to_string().contains("inserted"));
    assert_eq!(hex(&frame.payload), "11");
}

#[test]
fn crc16_matches_the_frame_recorded_from_a_real_device() {
    // LI/BF/MC, ID (LSB first), payload, auth of counter 1979 from device 00763DA3
    assert_eq!(crc16(&[0x07, 0xbb, 0xa3, 0x3d, 0x76, 0x00, 0x11, 0xab, 0x6d]), 0x873b);
}

// Downlink confirmation as sigfox-ep-lib builds it: CT 0x09, idle 3300 mV, TX 3150 mV, 23.5 °C, received at -72 dBm
const CONFIRMATION: [u8; 8] = [0x09, 0xe4, 0x0c, 0x4e, 0x0c, 0xeb, 0x00, 28];

#[test]
fn control_frames_are_decoded_with_the_control_frame_types() {
    let f = fields(2031, &CONFIRMATION);
    let auth = uplink_auth(&PUBLIC_KEY, &f);
    for rank in 0..3 {
        // the receiver starts somewhere in the preamble
        let frame = decode_uplink_bits(&encode_uplink(&f, rank, &auth, true)[4..]).unwrap();
        assert_eq!(
            (
                frame.rank,
                frame.device_id,
                frame.counter,
                frame.control,
                frame.crc_ok,
                frame.downlink
            ),
            (rank, DEVICE, 2031, true, true, false)
        );
        assert_eq!(frame.payload, CONFIRMATION);
        assert_eq!(frame.auth, auth);
    }
}

#[test]
fn data_frames_are_not_control_frames() {
    let f = fields(5, &[1, 2, 3, 4, 5, 6, 7, 8]);
    let frame = decode_uplink_bits(&encode_uplink(&f, 0, &uplink_auth(&PUBLIC_KEY, &f), false)[4..]).unwrap();
    assert_eq!((frame.control, frame.crc_ok), (false, true));
}

#[test]
fn control_payloads() {
    assert_eq!(
        decode_control(&CONFIRMATION),
        Some(ControlMessage {
            kind: ControlKind::DownlinkConfirmation,
            voltage_idle_mv: 3300,
            voltage_tx_mv: 3150,
            temperature_c: 23.5,
            downlink_rssi_dbm: Some(-72),
        })
    );
    // keep-alive: no RSSI, negative temperature
    assert_eq!(
        decode_control(&[0x08, 0xe4, 0x0c, 0x4e, 0x0c, 0x9c, 0xff]),
        Some(ControlMessage {
            kind: ControlKind::KeepAlive,
            voltage_idle_mv: 3300,
            voltage_tx_mv: 3150,
            temperature_c: -10.0,
            downlink_rssi_dbm: None,
        })
    );
    // received on air from an AX-SFJK after a downlink (seq 2046)
    assert_eq!(
        decode_control(&bytes("090c0d1b0d320116")),
        Some(ControlMessage {
            kind: ControlKind::DownlinkConfirmation,
            voltage_idle_mv: 3340,
            voltage_tx_mv: 3355,
            temperature_c: 30.6,
            downlink_rssi_dbm: Some(-78),
        })
    );
    // a weak downlink: RSSI + 100 is negative
    assert_eq!(
        decode_control(&[0x09, 0, 0, 0, 0, 0, 0, 0xf6])
            .unwrap()
            .downlink_rssi_dbm,
        Some(-110)
    );
    assert_eq!(decode_control(&[0x07, 0, 0, 0, 0, 0, 0, 0]), None);
    assert_eq!(decode_control(&[0x09, 0, 0]), None);
}
