//! Sigfox downlink frame, the inverse of SIGFOX_EP_BITSTREAM_decode_downlink_frame in sigfox-ep-lib:
//!   DL-PHY content (15 bytes) = ECC (4) | payload (8) | auth (2) | CRC8 (1), BCH(15,11)-coded column-wise
//!   and whitened with a PN sequence seeded by device ID * message counter.
//! On air: GFSK 600 bps, deviation 800 Hz, preamble then the sync word 0xB227, then the 15 bytes.

use crate::auth::aes_blocks;
use crate::frame::DeviceId;

pub const DL_SYNC_WORD: u16 = 0xb227;
pub const DL_BIT_RATE: u32 = 600;
pub const DL_DEVIATION_HZ: u32 = 800;
/// RC3: the downlink is sent on the frequency of the uplink's first frame + (f_dl - f_ul)
pub const RC3_UL_HZ: u32 = 923_200_000;
pub const RC3_DL_HZ: u32 = 922_200_000;
/// RC3: the device listens from T_W after the end of uplink frame 1 for T_RX (sigfox_ep_api.c starts the T_W
/// timer when frame rank 1 has been sent; RC3 uses SIGFOX_SPECTRUM_ACCESS_LBT_M80 in sigfox_rc.c)
pub const RC3_DL_T_W_MS: i64 = 19_000;
pub const RC3_DL_T_RX_MS: i64 = 33_500;
// Where to transmit inside that window: late enough for the error of the device's timer and of our estimate of
// the end of frame 1, and early enough that carrier sense can postpone the transmission
const TX_DELAY_MS: i64 = 2000;
const TX_END_MARGIN_MS: i64 = 1000;

const POW_ALPHA: [u8; 15] = [1, 2, 4, 8, 3, 6, 12, 11, 5, 10, 7, 14, 15, 13, 9];
const PAYLOAD: usize = 4;
const AUTH: usize = 12;
const CRC: usize = 14;

pub fn downlink_frequency(uplink_freq_hz: u32) -> u32 {
    uplink_freq_hz - (RC3_UL_HZ - RC3_DL_HZ)
}

/// Wall-clock ms: when to transmit, and the latest start that still ends inside the device's window
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownlinkWindow {
    pub at: i64,
    pub deadline: i64,
}

pub fn downlink_window(first_frame_end_at: i64) -> DownlinkWindow {
    let opens = first_frame_end_at + RC3_DL_T_W_MS;
    DownlinkWindow {
        at: opens + TX_DELAY_MS,
        deadline: opens + RC3_DL_T_RX_MS - TX_END_MARGIN_MS,
    }
}

pub fn crc8(data: &[u8]) -> u8 {
    let mut c: u8 = 0;
    for &b in data {
        c ^= b;
        for _ in 0..8 {
            c = if c & 0x80 != 0 { (c << 1) ^ 0x2f } else { c << 1 };
        }
    }
    c
}

pub fn downlink_auth(key: &[u8; 16], device_id: DeviceId, counter: u16, payload: &[u8; 8]) -> [u8; 2] {
    let id = device_id.0.to_le_bytes();
    let mut input = [0u8; 16];
    input[..4].copy_from_slice(&id);
    input[4] = counter as u8;
    input[5] = (counter >> 8 & 0x0f) as u8;
    input[6..14].copy_from_slice(payload);
    input[14..].copy_from_slice(&id[..2]);
    let out = aes_blocks(key, &input);
    [out[0], out[1]]
}

fn pn_step(mut pn: u32) -> u32 {
    for _ in 0..8 {
        let msb = pn & 0x21;
        pn = pn >> 1 | if msb == 0x20 || msb == 0x01 { 0x100 } else { 0 };
    }
    pn
}

/// XOR with the PN sequence: whitening and de-whitening are the same operation
pub fn whiten(phy: &[u8; 15], device_id: DeviceId, counter: u16) -> [u8; 15] {
    let mut buf = [0u8; 16];
    buf[..15].copy_from_slice(phy);
    let mut pn = ((u64::from(device_id.0) * u64::from(counter)) & 0x1ff) as u32;
    if pn == 0 {
        pn = 0x1ff;
    }
    // Each step's 9 PN bits straddle two bytes: the top bits end byte `at`, the low (idx + 1) bits start the next
    let mut apply = |pn: u32, at: usize, idx: usize| {
        buf[at] ^= (pn >> (idx + 1)) as u8;
        buf[at + 1] ^= ((pn & ((1 << (idx + 1)) - 1)) << (7 - idx)) as u8;
    };
    for idx in 0..8 {
        pn = pn_step(pn);
        apply(pn, idx, idx);
    }
    for idx in 0..5 {
        pn = pn_step(pn);
        apply(pn, idx + 9, idx);
    }
    pn = pn_step(pn);
    buf[14] ^= (pn >> 6) as u8;
    buf[..15].try_into().unwrap()
}

/// Fills bytes 0-3 so that every bit column (bit i of bytes 0..14) is a BCH(15,11) codeword: its syndrome,
/// the XOR of alpha^j over the set bits, is zero. alpha^0..alpha^3 are 1, 2, 4, 8, so the parity of
/// column i is simply the syndrome of its data bits.
fn add_bch(phy: &mut [u8; 15]) {
    phy[..PAYLOAD].fill(0);
    for i in 0..8 {
        let s = (PAYLOAD..15)
            .filter(|&j| phy[j] >> i & 1 == 1)
            .fold(0, |s, j| s ^ POW_ALPHA[j]);
        for (j, parity) in phy[..PAYLOAD].iter_mut().enumerate() {
            if s >> j & 1 == 1 {
                *parity |= 1 << i;
            }
        }
    }
}

pub fn encode_downlink(device_id: DeviceId, counter: u16, payload: &[u8; 8], key: &[u8; 16]) -> [u8; 15] {
    let mut phy = [0u8; 15];
    phy[PAYLOAD..AUTH].copy_from_slice(payload);
    phy[AUTH..CRC].copy_from_slice(&downlink_auth(key, device_id, counter, payload));
    phy[CRC] = crc8(&phy[PAYLOAD..CRC]);
    add_bch(&mut phy);
    whiten(&phy, device_id, counter)
}
