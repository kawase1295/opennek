//! Uplink authentication (UL-AUTH) as computed by sigfox-ep-lib (_aes_encrypt in sigfox_ep_bitstream.c):
//! AES-128-CBC with a zero IV over the LI/BF/REP/MC, EP ID and payload bytes repeated to fill 16-byte blocks
//! (two blocks for 11- and 12-byte payloads); the field is the first 2-5 bytes of the last cipher block.

use aes::Aes128;
use aes::cipher::{BlockEncrypt, KeyInit};

use crate::frame::{UplinkFields, uplink_header};

/// SIGFOX_EP_PUBLIC_KEY, used by devices switched to public key mode (e.g. ATS410=1)
pub const PUBLIC_KEY: [u8; 16] = [
    0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
];

/// AES-128-CBC with a zero IV and no padding; `data` is a whole number of blocks
pub(crate) fn aes_blocks(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    debug_assert_eq!(data.len() % 16, 0, "AES input must be whole blocks");
    let cipher = Aes128::new(key.into());
    let mut prev = [0u8; 16];
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.as_chunks::<16>().0 {
        let mut block = std::array::from_fn::<u8, 16, _>(|i| chunk[i] ^ prev[i]).into();
        cipher.encrypt_block(&mut block);
        prev = block.into();
        out.extend(prev);
    }
    out
}

pub fn uplink_auth(key: &[u8; 16], f: &UplinkFields) -> Vec<u8> {
    let header = uplink_header(f);
    let blocks = if f.payload.len() >= 11 { 2 } else { 1 };
    let input: Vec<u8> = header.bytes.iter().copied().cycle().take(16 * blocks).collect();
    let last = 16 * (blocks - 1);
    aes_blocks(key, &input)[last..last + header.auth_size].to_vec()
}

pub fn verify_uplink(key: &[u8; 16], f: &UplinkFields, auth: &[u8]) -> bool {
    uplink_auth(key, f) == auth
}
