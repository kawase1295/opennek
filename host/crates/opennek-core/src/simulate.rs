use serde_json::Value;
use sigfox::auth::uplink_auth;
use sigfox::frame::{DeviceId, UplinkFields};
use sigfox::receiver::SigfoxMessage;

use crate::radio::RadioMessage;

const MAX_PAYLOAD_BYTES: usize = 12;
const DEFAULT_RSSI: i32 = -100;

/// An uplink to simulate, as POST /api/uplinks takes it
#[derive(Clone, Debug, PartialEq)]
pub struct UplinkInput {
    pub device_id: DeviceId,
    pub data: Vec<u8>,
    /// The device asks for a downlink
    pub ack: bool,
    /// None: the next after the device's last
    pub seq: Option<u16>,
    /// dBm
    pub rssi: i32,
    /// The key to sign with in place of the device's, so that a key mismatch can be tried
    pub key: Option<[u8; 16]>,
}

fn hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

impl UplinkInput {
    /// Optional fields that are left out or null take their defaults
    pub fn parse(body: Value) -> Result<UplinkInput, String> {
        let Value::Object(fields) = body else {
            return Err("body: expected an object".into());
        };
        let field = |k: &str| fields.get(k).filter(|v| !v.is_null());
        let device_id = field("device")
            .and_then(Value::as_str)
            .filter(|s| s.len() == 8 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|s| s.parse().ok())
            .ok_or("device: 8 hex digits")?;
        let data = field("data")
            .and_then(Value::as_str)
            .and_then(hex)
            .filter(|d| d.len() <= MAX_PAYLOAD_BYTES)
            .ok_or_else(|| format!("data: hex, at most {MAX_PAYLOAD_BYTES} bytes"))?;
        let ack = match field("ack") {
            Some(v) => v.as_bool().ok_or("ack: true or false")?,
            None => false,
        };
        let seq = field("seq")
            .map(|v| {
                v.as_u64()
                    .filter(|&s| s <= 0xfff)
                    .map(|s| s as u16)
                    .ok_or("seq: an integer from 0 to 4095")
            })
            .transpose()?;
        let rssi = field("rssi")
            .map(|v| {
                v.as_i64()
                    .filter(|r| (-200..=0).contains(r))
                    .map(|r| r as i32)
                    .ok_or("rssi: an integer from -200 to 0 (dBm)")
            })
            .transpose()?
            .unwrap_or(DEFAULT_RSSI);
        let key = field("key")
            .map(|v| {
                v.as_str()
                    .and_then(hex)
                    .and_then(|k| k.try_into().ok())
                    .ok_or("key: 16 bytes of hex")
            })
            .transpose()?;
        Ok(UplinkInput {
            device_id,
            data,
            ack,
            seq,
            rssi,
            key,
        })
    }
}

pub(crate) fn radio_message(input: &UplinkInput, seq: u16, key: &[u8; 16], received_at: i64) -> RadioMessage {
    let fields = UplinkFields {
        device_id: input.device_id,
        counter: seq,
        payload: &input.data,
        bit: None,
        downlink: input.ack,
    };
    RadioMessage {
        message: SigfoxMessage {
            device_id: input.device_id,
            counter: seq,
            payload: input.data.clone(),
            bit: None,
            downlink: input.ack,
            control: false,
            auth: uplink_auth(key, &fields),
            time: 0.0,
            first_frame_end: None,
            rssi: input.rssi as f32,
            ranks: vec![0, 1, 2],
            frames: vec![],
            clock_error_ppm: 0,
        },
        received_at,
        first_frame_end_at: None,
        uplink_freq_hz: None,
    }
}
