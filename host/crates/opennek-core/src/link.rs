//! How well a device gets through: its link quality over its stored messages

use serde::Serialize;

/// GET /api/devices/{id}/stats: a device's link quality over its stored messages; null when it has none
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LinkStats {
    pub messages: u32,
    /// Wall-clock ms of the oldest and newest
    pub first: i64,
    pub last: i64,
    pub rssi: RssiSummary,
    /// Messages received with 1, 2 and 3 of the 3 frames: [0] is 1/3
    pub frames: [u32; 3],
    pub auth: AuthCounts,
    /// Uplinks missed, counted from the gaps in seq
    pub missed: u32,
    /// Each message's time and RSSI, oldest first
    pub points: Vec<RssiPoint>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RssiSummary {
    pub min: i32,
    pub avg: f64,
    pub max: i32,
}

/// Messages by how their authentication checked out (store::Auth)
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AuthCounts {
    pub ok: u32,
    pub bad: u32,
    pub public: u32,
    pub unverified: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RssiPoint {
    /// Wall-clock ms
    pub time: i64,
    pub rssi: i32,
}

/// Sigfox sequence numbers are 12 bits
const SEQ_MODULO: u16 = 4096;
/// The largest jump ahead counted as loss; a larger one is a device that restarted its counter or was away
const MAX_GAP: u16 = 256;
/// How far behind the last seq an uplink still counts as late or out of order, not as a restarted counter
const MAX_LATE: u16 = 16;

/// The uplinks missed between `seqs`, in the order they were received
pub fn missed_uplinks(seqs: impl IntoIterator<Item = u16>) -> u32 {
    let mut seqs = seqs.into_iter();
    let Some(mut last) = seqs.next() else {
        return 0;
    };
    let mut missed = 0;
    for seq in seqs {
        let ahead = seq.wrapping_sub(last) % SEQ_MODULO;
        if ahead == 0 || SEQ_MODULO - ahead <= MAX_LATE {
            continue;
        }
        if ahead <= MAX_GAP {
            missed += u32::from(ahead - 1);
        }
        last = seq;
    }
    missed
}
