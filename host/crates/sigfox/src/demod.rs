//! Envelope (RSSI) demodulator for Sigfox DBPSK uplinks.
//! Pulse-shaped DBPSK makes the amplitude dip towards zero at every phase reversal, so the dips alone give
//! the differentially decoded bits: a dip on a bit boundary means data bit 0, no dip means 1.

use std::collections::HashSet;
use std::f64::consts::PI;

use crate::frame::{UplinkFrame, decode_uplink_bits};

pub const BIT_RATE: u32 = 100;

/// Sample indices of one burst and its levels in dBm
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Burst {
    pub start: usize,
    pub end: usize,
    pub peak_dbm: f32,
    pub floor_dbm: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct BurstOptions {
    pub above_db: f64,
    pub min_ms: f64,
    pub gap_ms: f64,
}

impl Default for BurstOptions {
    fn default() -> Self {
        Self {
            above_db: 15.0,
            min_ms: 300.0,
            gap_ms: 40.0,
        }
    }
}

// Math.round of the TypeScript host: halves go up, also below zero
fn round(x: f64) -> f64 {
    (x + 0.5).floor()
}

// The value at `q` (0..1) of the sorted samples, indexed as floor(len * q) like the TypeScript host
fn quantile(xs: &[f32], q: f64) -> Option<f32> {
    let mut s = xs.to_vec();
    s.sort_by(f32::total_cmp);
    s.get((s.len() as f64 * q).floor() as usize).copied()
}

pub fn noise_floor(dbm: &[f32]) -> f32 {
    quantile(dbm, 0.2).unwrap_or(-120.0)
}

/// Frames last over a second and sit well above the noise; the dips inside a frame are short gaps
pub fn find_bursts(dbm: &[f32], rate: u32, opts: &BurstOptions, floor: f32) -> Vec<Burst> {
    let rate = f64::from(rate);
    let threshold = f64::from(floor) + opts.above_db;
    let gap = round(opts.gap_ms / 1000.0 * rate) as usize;
    let mut bursts = vec![];
    let mut start = None;
    let mut last_above = 0;
    for i in 0..=dbm.len() {
        if i < dbm.len() && f64::from(dbm[i]) > threshold {
            start.get_or_insert(i);
            last_above = i;
        } else if let Some(s) = start
            && (i == dbm.len() || i - last_above > gap)
        {
            if (last_above - s) as f64 / rate >= opts.min_ms / 1000.0 {
                let peak = dbm[s..=last_above].iter().fold(-200f32, |m, &v| m.max(v));
                bursts.push(Burst {
                    start: s,
                    end: last_above + 1,
                    peak_dbm: peak,
                    floor_dbm: floor,
                });
            }
            start = None;
        }
    }
    bursts
}

#[derive(Clone, Debug, PartialEq)]
pub struct BurstBits {
    pub bits: Vec<bool>,
    pub period_samples: f64,
    pub clock_error_ppm: i32,
    pub level_dbm: f32,
    pub dips: usize,
}

/// Dip centres: local minima at least 6 dB below both sides, half a bit apart, refined to sub-sample
fn find_dips(seg: &[f32], nominal: f64, level: f64) -> Vec<f64> {
    // Dips are measured against the local envelope on each side (max over 1.5 bits). A real dip has a high
    // level on both sides; this rejects the rising and falling edges of the frame, and copes with the
    // transmit power ramping during the first part of a frame.
    let win = round(nominal * 1.5) as usize;
    let side_max = |from: usize, to: usize| {
        seg[from..=to.min(seg.len() - 1)]
            .iter()
            .fold(-200f64, |m, &v| m.max(v.into()))
    };
    let half = round(nominal * 0.4) as usize;
    let mut dips = vec![];
    for i in 1..seg.len().saturating_sub(1) {
        let v = f64::from(seg[i]);
        let left = side_max(i.saturating_sub(win), i - 1);
        let right = side_max(i + 1, i + win);
        if v >= left.min(right) - 6.0 || left.max(right) < level - 25.0 {
            continue;
        }
        let around = i.saturating_sub(half)..=(i + half).min(seg.len() - 1);
        if around
            .into_iter()
            .any(|k| f64::from(seg[k]) < v || (f64::from(seg[k]) == v && k < i))
        {
            continue;
        }
        let a = f64::from(seg[i - 1]);
        let c = f64::from(seg[i + 1]);
        let denom = a - 2.0 * v + c;
        dips.push(i as f64 + if denom != 0.0 { 0.5 * (a - c) / denom } else { 0.0 });
    }
    dips
}

/// Finds the dips of one burst, recovers the bit clock from their positions and returns the data bits
pub fn burst_bits(seg: &[f32], rate: u32) -> Option<BurstBits> {
    let nominal = f64::from(rate) / f64::from(BIT_RATE);
    let level = quantile(seg, 0.9)?;
    let dips = find_dips(seg, nominal, level.into());
    if dips.len() < 8 {
        return None;
    }

    // Bit clock: assign each dip to a bit index with the current estimate, then refit period and phase by
    // least squares. Tracking dip by dip keeps a 1 % clock error from slipping a whole bit over a frame.
    let mut period = nominal;
    // Anchor the grid on the mean phase of the first dips (circular mean modulo one bit)
    let (s, c) = dips.iter().take(12).fold((0.0, 0.0), |(s, c), d| {
        let a = 2.0 * PI * (d - dips[0]) / nominal;
        (s + a.sin(), c + a.cos())
    });
    let mut phase = dips[0] + f64::atan2(s, c) / (2.0 * PI) * nominal;
    let mut points: Vec<(i64, f64)> = vec![];
    for &d in &dips {
        let k = round((d - phase) / period) as i64;
        let predicted = phase + k as f64 * period;
        if (d - predicted).abs() > 0.35 * period || points.last().is_some_and(|&(pk, _)| k <= pk) {
            continue;
        }
        points.push((k, d));
        if points.len() >= 4 {
            let (mut sk, mut sd, mut skk, mut skd) = (0.0, 0.0, 0.0, 0.0);
            for &(pk, pd) in &points {
                let pk = pk as f64;
                sk += pk;
                sd += pd;
                skk += pk * pk;
                skd += pk * pd;
            }
            let n = points.len() as f64;
            let det = n * skk - sk * sk;
            if det > 0.0 {
                period = (n * skd - sk * sd) / det;
                phase = (sd - period * sk) / n;
            }
        }
    }
    if points.len() < 8 {
        return None;
    }
    let first = points[0].0;
    let last = points[points.len() - 1].0;
    let on_grid: HashSet<i64> = points.iter().map(|&(k, _)| k).collect();
    // Bits after the last dip are 1s (no reversal); count them up to where the signal ends
    let mut last_high = seg.len() - 1;
    while last_high > 0 && f64::from(seg[last_high]) < f64::from(level) - 15.0 {
        last_high -= 1;
    }
    let end = last.max(((last_high as f64 - phase) / period).floor() as i64);
    // The first preamble bit has no phase reference
    let bits = std::iter::once(true)
        .chain((first..=end).map(|k| !on_grid.contains(&k)))
        .collect();
    Some(BurstBits {
        bits,
        period_samples: period,
        clock_error_ppm: round((period / nominal - 1.0) * 1e6) as i32,
        level_dbm: level,
        dips: points.len(),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedFrame {
    pub frame: UplinkFrame,
    /// Start and end of the burst, in seconds
    pub time: f64,
    pub end: f64,
    pub rssi: f32,
    pub clock_error_ppm: i32,
}

pub fn decode_burst(dbm: &[f32], rate: u32, b: &Burst, t0: f64) -> Option<DecodedFrame> {
    let pad = round(f64::from(rate) * 0.02) as usize;
    // Clamped like a TypeScript subarray: a burst outside the samples gives an empty segment, which has no bits
    let from = b.start.saturating_sub(pad).min(dbm.len());
    let seg = &dbm[from..b.end.saturating_add(pad).clamp(from, dbm.len())];
    let bits = burst_bits(seg, rate)?;
    let frame = decode_uplink_bits(&bits.bits)?;
    let rate = f64::from(rate);
    Some(DecodedFrame {
        frame,
        time: t0 + b.start as f64 / rate,
        end: t0 + b.end as f64 / rate,
        rssi: b.peak_dbm,
        clock_error_ppm: bits.clock_error_ppm,
    })
}

/// Decodes every frame found in a block of RSSI samples (dBm). `t0` is the time of the first sample in seconds.
pub fn decode_capture(dbm: &[f32], rate: u32, t0: f64, floor: Option<f32>) -> Vec<DecodedFrame> {
    let floor = floor.unwrap_or_else(|| noise_floor(dbm));
    find_bursts(dbm, rate, &BurstOptions::default(), floor)
        .iter()
        .filter_map(|b| decode_burst(dbm, rate, b, t0))
        .collect()
}
