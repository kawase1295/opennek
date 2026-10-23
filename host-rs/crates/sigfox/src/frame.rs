//! Sigfox uplink frame layout, as built by sigfox-ep-lib (github.com/sigfox-tech-radio/sigfox-ep-lib):
//!   PR (0xAAAA) | FT (16 bit) | LI(2) BF(1) REP(1) MC(12) | EP ID (4, LSB first) | payload | auth | CRC16
//! Repetitions 2 and 3 (ranks 1 and 2 here) convolve everything after FT.

use std::fmt;
use std::str::FromStr;

pub const PREAMBLE: u16 = 0xaaaa;
/// Frame types per repetition rank, indexed by payload class: empty/bit, 1 byte, 2-4, 5-8, 9-12 bytes
pub const FRAME_TYPES: [[u16; 5]; 3] = [
    [0xa06b, 0xa08d, 0xa35f, 0xa611, 0xa94c],
    [0xa6e0, 0xa0d2, 0xa598, 0xa6bf, 0xa971],
    [0xa034, 0xa302, 0xa5a3, 0xa72c, 0xa997],
];
/// Control frames (keep-alive, downlink confirmation): 7 or 8 bytes, laid out like payload class 3
pub const CONTROL_FRAME_TYPES: [u16; 3] = [0xaf67, 0xafc9, 0xb1be];
const CONTROL_CLASS: usize = 3;
const CONV_NEXT: [usize; 4] = [0, 0, 1, 1];
const CONV_OUT: [[bool; 4]; 3] = [
    [false, false, false, false],
    [false, true, true, false],
    [false, true, false, true],
];

/// The device ID as printed by AT$ID: 8 hex digits
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeviceId(pub u32);

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08X}", self.0)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseDeviceIdError;

impl FromStr for DeviceId {
    type Err = ParseDeviceIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() || s.len() > 8 {
            return Err(ParseDeviceIdError);
        }
        u32::from_str_radix(s, 16).map(DeviceId).map_err(|_| ParseDeviceIdError)
    }
}

/// The fields an uplink carries, which its authentication covers
#[derive(Clone, Copy, Debug)]
pub struct UplinkFields<'a> {
    pub device_id: DeviceId,
    /// Message counter (12 bits)
    pub counter: u16,
    pub payload: &'a [u8],
    /// Single-bit messages (AT$SB)
    pub bit: Option<bool>,
    /// BF: the device waits for a downlink
    pub downlink: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UplinkFrame {
    /// 0, 1, 2 = first, second, third repetition
    pub rank: usize,
    pub device_id: DeviceId,
    pub counter: u16,
    pub payload: Vec<u8>,
    pub bit: Option<bool>,
    pub downlink: bool,
    /// A control frame (see [`decode_control`]), not application data
    pub control: bool,
    pub auth: Vec<u8>,
    pub crc_ok: bool,
    /// The bit-slip correction that made the CRC pass, if any
    pub repaired: Option<Repair>,
}

impl UplinkFrame {
    pub fn fields(&self) -> UplinkFields<'_> {
        UplinkFields {
            device_id: self.device_id,
            counter: self.counter,
            payload: &self.payload,
            bit: self.bit,
            downlink: self.downlink,
        }
    }
}

/// A single bit error in the frame body, at a bit index after FT
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repair {
    Inserted { bit: bool, at: usize },
    Deleted { at: usize },
    Flipped { at: usize },
}

impl fmt::Display for Repair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Repair::Inserted { bit, at } => write!(f, "inserted {} at {at}", u8::from(*bit)),
            Repair::Deleted { at } => write!(f, "deleted bit {at}"),
            Repair::Flipped { at } => write!(f, "flipped bit {at}"),
        }
    }
}

pub fn crc16(data: &[u8]) -> u16 {
    let mut c: u16 = 0;
    for &b in data {
        c ^= u16::from(b) << 8;
        for _ in 0..8 {
            c = if c & 0x8000 != 0 { (c << 1) ^ 0x1021 } else { c << 1 };
        }
    }
    c ^ 0xffff
}

fn push_bits(out: &mut Vec<bool>, v: u16, n: u32) {
    out.extend((0..n).rev().map(|i| v >> i & 1 == 1));
}

fn bits_value(bits: &[bool]) -> u16 {
    bits.iter().fold(0, |v, &b| v << 1 | u16::from(b))
}

fn to_bytes(bits: &[bool]) -> Vec<u8> {
    bits.as_chunks::<8>().0.iter().map(|c| bits_value(c) as u8).collect()
}

pub fn convolve(bits: &[bool], rank: usize) -> Vec<bool> {
    let mut state = 0;
    bits.iter()
        .map(|&input| {
            let out = input ^ CONV_OUT[rank][state];
            state = CONV_NEXT[state] + if input { 2 } else { 0 };
            out
        })
        .collect()
}

pub fn deconvolve(bits: &[bool], rank: usize) -> Vec<bool> {
    let mut state = 0;
    bits.iter()
        .map(|&b| {
            let input = b ^ CONV_OUT[rank][state];
            state = CONV_NEXT[state] + if input { 2 } else { 0 };
            input
        })
        .collect()
}

/// Payload size and auth size for a payload class and the LI read from the frame; None if they contradict
fn sizes(class: usize, li: usize) -> Option<(usize, usize)> {
    match class {
        0 => Some((0, 2)),
        1 => Some((1, 2)),
        _ => (4 * (class - 1)).checked_sub(li).map(|payload| (payload, li + 2)),
    }
}

/// LI/BF/REP/MC, EP ID (LSB first) and payload: the part of an uplink covered by the authentication field
pub struct UplinkHeader {
    pub bytes: Vec<u8>,
    pub class: usize,
    pub auth_size: usize,
}

pub fn uplink_header(f: &UplinkFields) -> UplinkHeader {
    let n = f.payload.len();
    let class = match (f.bit, n) {
        (Some(_), _) | (None, 0) => 0,
        (None, 1) => 1,
        _ => (n - 1) / 4 + 2,
    };
    let li = match f.bit {
        Some(bit) => 2 + usize::from(bit),
        None if class >= 2 => 3 - (n - 1) % 4,
        None => 0,
    };
    let mut bytes = Vec::with_capacity(6 + n);
    bytes.push((li << 6) as u8 | u8::from(f.downlink) << 5 | (f.counter >> 8 & 0x0f) as u8);
    bytes.push(f.counter as u8);
    bytes.extend(f.device_id.0.to_le_bytes());
    bytes.extend(f.payload);
    UplinkHeader {
        bytes,
        class,
        auth_size: if class >= 2 { li + 2 } else { 2 },
    }
}

/// Builds the data bits of an uplink frame, preamble included
pub fn encode_uplink(f: &UplinkFields, rank: usize, auth: &[u8], control: bool) -> Vec<bool> {
    let header = uplink_header(f);
    let body = [header.bytes.as_slice(), auth].concat();
    let mut data = Vec::new();
    for &b in &body {
        push_bits(&mut data, b.into(), 8);
    }
    push_bits(&mut data, crc16(&body), 16);
    if rank > 0 {
        data = convolve(&data, rank);
    }
    let ft = if control {
        CONTROL_FRAME_TYPES[rank]
    } else {
        FRAME_TYPES[rank][header.class]
    };
    let mut bits = Vec::with_capacity(32 + data.len());
    push_bits(&mut bits, PREAMBLE, 16);
    push_bits(&mut bits, ft, 16);
    bits.extend(data);
    bits
}

fn parse_body(
    body_bits: &[bool],
    rank: usize,
    class: usize,
    control: bool,
    repaired: Option<Repair>,
) -> Option<UplinkFrame> {
    let plain = if rank > 0 {
        deconvolve(body_bits, rank)
    } else {
        body_bits.to_vec()
    };
    if plain.len() < 16 {
        return None;
    }
    let li = usize::from(bits_value(&plain[..2]));
    let (payload, auth) = sizes(class, li)?;
    let body_bytes = 2 + 4 + payload + auth;
    let total = (body_bytes + 2) * 8;
    if plain.len() < total {
        return None;
    }
    let bytes = to_bytes(&plain[..total]);
    let body = &bytes[..body_bytes];
    let crc = u16::from_be_bytes([bytes[body_bytes], bytes[body_bytes + 1]]);
    Some(UplinkFrame {
        rank,
        device_id: DeviceId(u32::from_le_bytes([body[2], body[3], body[4], body[5]])),
        counter: u16::from(body[0] & 0x0f) << 8 | u16::from(body[1]),
        payload: body[6..6 + payload].to_vec(),
        bit: (class == 0 && li >= 2).then_some(li == 3),
        downlink: body[0] & 0x20 != 0,
        control,
        auth: body[6 + payload..].to_vec(),
        crc_ok: crc16(body) == crc,
        repaired,
    })
}

/// The single-bit errors tried at bit `i` of a body that fails its CRC
fn repairs(body: &[bool], i: usize) -> [(Vec<bool>, Repair); 4] {
    let with = |middle: &[bool], skip: usize| [&body[..i], middle, &body[i + skip..]].concat();
    [
        (with(&[true], 0), Repair::Inserted { bit: true, at: i }),
        (with(&[false], 0), Repair::Inserted { bit: false, at: i }),
        (with(&[], 1), Repair::Deleted { at: i }),
        (with(&[!body[i]], 1), Repair::Flipped { at: i }),
    ]
}

/// Decodes data bits that start somewhere in the preamble. The frame type is searched near where it should
/// be, because the first preamble bits are not observable and the leading edge can add a spurious bit.
/// When the CRC fails, single slipped, extra or flipped bits are tried (the receiver clock drifts).
pub fn decode_uplink_bits(bits: &[bool]) -> Option<UplinkFrame> {
    let mut start = 8;
    while start <= 22 && start + 16 <= bits.len() {
        let ft = bits_value(&bits[start..start + 16]);
        for rank in 0..3 {
            let control = ft == CONTROL_FRAME_TYPES[rank];
            let Some(class) = (if control {
                Some(CONTROL_CLASS)
            } else {
                FRAME_TYPES[rank].iter().position(|&t| t == ft)
            }) else {
                continue;
            };
            let body = &bits[start + 16..];
            let frame = parse_body(body, rank, class, control, None);
            if frame.as_ref().is_some_and(|f| f.crc_ok) {
                return frame;
            }
            // One bit lost or gained somewhere (clock slip), or one bit wrong
            for i in 0..body.len() {
                for (candidate, how) in repairs(body, i) {
                    if let Some(fixed) = parse_body(&candidate, rank, class, control, Some(how)).filter(|f| f.crc_ok) {
                        return Some(fixed);
                    }
                }
            }
            return frame;
        }
        start += 1;
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    KeepAlive,
    DownlinkConfirmation,
}

/// Payload of a control frame (sigfox-ep-lib SIGFOX_EP_BITSTREAM_build_control_frame): a type byte, then little-endian
/// voltages and temperature; a downlink confirmation adds the RSSI at which the device received the downlink
#[derive(Clone, Debug, PartialEq)]
pub struct ControlMessage {
    pub kind: ControlKind,
    pub voltage_idle_mv: u16,
    pub voltage_tx_mv: u16,
    pub temperature_c: f64,
    pub downlink_rssi_dbm: Option<i16>,
}

pub fn decode_control(payload: &[u8]) -> Option<ControlMessage> {
    let kind = match (payload.first(), payload.len()) {
        (Some(0x08), 7) => ControlKind::KeepAlive,
        (Some(0x09), 8) => ControlKind::DownlinkConfirmation,
        _ => return None,
    };
    let u16_at = |i: usize| u16::from_le_bytes([payload[i], payload[i + 1]]);
    Some(ControlMessage {
        kind,
        voltage_idle_mv: u16_at(1),
        voltage_tx_mv: u16_at(3),
        temperature_c: f64::from(i16::from_le_bytes([payload[5], payload[6]])) / 10.0,
        downlink_rssi_dbm: (kind == ControlKind::DownlinkConfirmation).then(|| i16::from(payload[7] as i8) - 100),
    })
}
