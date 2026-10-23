//! Frequency of uplink frame 1, which the downlink frequency is derived from (#6), measured with the GFSK carrier
//! frequency offset (CFO) register that the LSM100A streamer reports from fw 0.2.0 on.
//!
//! The register reads about 7 % low (CFO_SCALE), so a reading far from the receiver frequency is only good to a
//! few hundred Hz even after correcting for that. To get well within the device's 4.8 kHz receive bandwidth, the
//! receiver is tuned to the coarse frame 1 reading before frame 2 starts. Frames 2 and 3 (frame 1 +/- 6 kHz,
//! sigfox-ep-lib SIGFOX_EP_FREQUENCY_compute_uplink) then land close to the receiver frequency, and frame 1 is
//! their midpoint; on hardware this agreed with the direct reading within 122 Hz.

const XTAL_HZ: f64 = 32_000_000.0;
/// The streamer's GFSK setting (firmware-lsm100a configureRx)
pub const STREAMER_BIT_RATE: u32 = 50_000;
/// One CFO value per 8 RSSI samples
pub const CFO_EVERY: usize = 8;
pub const RC3_DELTA_F_MF_HZ: f64 = 6000.0;
/// Measured on hardware (LSM100A, RX_BW 234 kHz): one frame read 50 171 Hz from one receiver frequency and 3 784 Hz
/// from another 50 171 Hz higher, so the register gives 0.925 of the offset; frames 2/3 came out 11.1-11.4 kHz
/// apart instead of 12 kHz (0.926-0.946)
pub const CFO_SCALE: f64 = 0.93;

/// A sample this far above the noise floor counts as signal
const ABOVE_DB: f64 = 15.0;
/// CFO readings are trusted this long into a frame and after a retune
const SETTLE_S: f64 = 0.3;
/// A longer gap ends a frame (the DBPSK dips are shorter)
const GAP_S: f64 = 0.1;
/// A longer gap ends a message: back to the base frequency
const QUIET_S: f64 = 10.0;
const KEEP_S: f64 = 60.0;
// The scale is known to about 1 %, and only from readings up to 50 kHz: a message read further than this from the
// receiver frequency gets no frequency rather than one that may miss the device's window (+/- 1 kHz is the aim)
const MAX_OFFSET_HZ: f64 = 10_000.0;

/// Hz per CFO LSB, as in SUBGRF_GetCFO (STM32CubeWL): the RX_BW code has the BWSELR layout
pub fn cfo_step_hz(bw_code: u8, bit_rate: u32) -> f64 {
    let mant = [4.0, 8.0, 10.0, 12.0][usize::from(bw_code >> 3 & 3)];
    let cf_fs = XTAL_HZ / (mant * f64::from(1u32 << ((bw_code & 7) + 1)));
    let osr = (cf_fs / f64::from(bit_rate)).floor();
    let interp = if osr < 4.0 {
        4.0
    } else if osr < 8.0 {
        2.0
    } else {
        1.0
    };
    cf_fs * interp / 4096.0
}

/// A raw 12-bit register value as a signed offset
pub fn cfo_signed(v: u16) -> i32 {
    let v = i32::from(v & 0xfff);
    if v & 0x800 != 0 { v - 0x1000 } else { v }
}

fn median(xs: &[f64]) -> f64 {
    let mut s = xs.to_vec();
    s.sort_by(f64::total_cmp);
    s[s.len() / 2]
}

// Math.round of the TypeScript host: halves go up
fn round_hz(x: f64) -> u32 {
    (x + 0.5).floor() as u32
}

/// When one frame was on air, in stream seconds
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameSpan {
    pub rank: usize,
    pub time: f64,
    pub end: f64,
}

/// A frame's frequency and the receiver frequency it was measured against
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameFrequency {
    pub hz: f64,
    pub lo: u32,
}

#[derive(Clone, Copy, Debug)]
struct Reading {
    at: i64,
    /// Receiver frequency
    lo: u32,
    /// lo + CFO
    hz: f64,
}

/// Follows the stream sample by sample. Sample indices count from the start of the stream, like the receiver.
#[derive(Debug)]
pub struct FrequencyTracker {
    rate: f64,
    base_freq: u32,
    step_hz: f64,
    /// Samples so far
    n: i64,
    line_start: i64,
    /// (sample index, receiver frequency) from each status line that changed it
    tuning: Vec<(i64, u32)>,
    readings: Vec<Reading>,
    floor: Option<f64>,
    frame_from: i64,
    last_signal: Option<i64>,
    retuned: bool,
    requested: Option<u32>,
}

impl FrequencyTracker {
    pub fn new(rate: u32, base_freq: u32, step_hz: f64) -> Self {
        Self {
            rate: f64::from(rate),
            base_freq,
            step_hz,
            n: 0,
            line_start: 0,
            tuning: vec![],
            readings: vec![],
            floor: None,
            frame_from: -1,
            last_signal: None,
            retuned: false,
            requested: None,
        }
    }

    pub fn freq(&self) -> u32 {
        self.tuning.last().map_or(self.base_freq, |&(_, f)| f)
    }

    fn since_signal(&self) -> f64 {
        self.last_signal.map_or(f64::INFINITY, |s| (self.n - s) as f64)
    }

    /// The streamer's status line reports the frequency it is tuned to
    pub fn tuned(&mut self, freq: u32) {
        if self.requested == Some(freq) {
            self.requested = None;
        }
        if freq != self.freq() || self.tuning.is_empty() {
            self.tuning.push((self.n, freq));
        }
    }

    /// One R line of RSSI samples (dBm)
    pub fn samples(&mut self, dbm: &[f32]) {
        self.line_start = self.n;
        for &v in dbm {
            let v = f64::from(v);
            let floor = *self.floor.get_or_insert(v);
            if v > floor + ABOVE_DB {
                if self.since_signal() > QUIET_S * self.rate {
                    self.retuned = false; // a new message
                }
                if self.since_signal() > GAP_S * self.rate {
                    self.frame_from = self.n;
                }
                self.last_signal = Some(self.n);
            } else {
                self.floor = Some(floor + (v - floor) / self.rate); // about one second to follow the noise floor
            }
            self.n += 1;
        }
        let keep = self.n as f64 - KEEP_S * self.rate;
        self.readings.retain(|r| r.at as f64 >= keep);
        // A tuning stays while it is the last one or the next one is still kept
        let next_at: Vec<Option<i64>> = self
            .tuning
            .iter()
            .skip(1)
            .map(|&(at, _)| Some(at))
            .chain([None])
            .collect();
        let mut next = next_at.into_iter();
        self.tuning
            .retain(|_| next.next().flatten().is_none_or(|at| at as f64 >= keep));
    }

    /// The C line for the last R line: raw 12-bit register values
    pub fn cfo(&mut self, raw: &[u16]) {
        let settle = SETTLE_S * self.rate;
        let tuned_at = self.tuning.last().map_or(0, |&(at, _)| at);
        let lo = self.freq();
        for (k, &v) in raw.iter().enumerate() {
            let at = self.line_start + (k * CFO_EVERY) as i64;
            if (at as f64) < tuned_at as f64 + settle && self.tuning.len() > 1 {
                continue; // the register is still catching up
            }
            let hz = f64::from(lo) + f64::from(cfo_signed(v)) * self.step_hz / CFO_SCALE;
            self.readings.push(Reading { at, lo, hz });
        }
    }

    /// Where to tune now, or None: to the coarse frame 1 reading once it has settled, and back to the base
    /// frequency after a message
    pub fn next_tuning(&mut self) -> Option<u32> {
        if self.requested.is_some() {
            return None;
        }
        if self.since_signal() > QUIET_S * self.rate {
            return self.request((self.freq() != self.base_freq).then_some(self.base_freq));
        }
        let settle = SETTLE_S * self.rate;
        if self.retuned || self.since_signal() > GAP_S * self.rate || ((self.n - self.frame_from) as f64) < settle {
            return None;
        }
        let from = self.frame_from as f64 + self.rate * GAP_S;
        let hz: Vec<f64> = self
            .readings
            .iter()
            .filter(|r| r.at as f64 >= from)
            .map(|r| r.hz)
            .collect();
        if hz.len() < 5 {
            return None;
        }
        self.retuned = true;
        let target = round_hz(median(&hz));
        self.request((target != self.freq()).then_some(target))
    }

    fn request(&mut self, freq: Option<u32>) -> Option<u32> {
        if freq.is_some() {
            self.requested = freq;
        }
        freq
    }

    /// Frequency of one frame against the last receiver frequency used during it (the closest one after a retune)
    pub fn frame_frequency(&self, f: &FrameSpan) -> Option<FrameFrequency> {
        let from = ((f.time + SETTLE_S) * self.rate + 0.5).floor() as i64;
        let to = (f.end * self.rate + 0.5).floor() as i64;
        let in_frame: Vec<&Reading> = self.readings.iter().filter(|r| r.at >= from && r.at < to).collect();
        let lo = in_frame.last()?.lo;
        let hz: Vec<f64> = in_frame.iter().filter(|r| r.lo == lo).map(|r| r.hz).collect();
        (hz.len() >= 5).then(|| FrameFrequency { hz: median(&hz), lo })
    }

    /// Frame 1 of a message: the midpoint of frames 2 and 3 when the device sent them at frame 1 +/- 6 kHz and they
    /// were measured with the same receiver frequency, otherwise frame 1 itself. None unless it was measured close
    /// to the receiver frequency.
    pub fn frame1(&self, frames: &[FrameSpan], downlink: bool) -> Option<u32> {
        let at = |rank| {
            frames
                .iter()
                .find(|x| x.rank == rank)
                .and_then(|f| self.frame_frequency(f))
        };
        if downlink
            && let (Some(f2), Some(f3)) = (at(1), at(2))
            && f2.lo == f3.lo
        {
            // a plausible spacing (12 kHz) guards against a frame from another device
            let spacing = f2.hz - f3.hz;
            let mid = (f2.hz + f3.hz) / 2.0;
            if spacing > 1.5 * RC3_DELTA_F_MF_HZ
                && spacing < 2.5 * RC3_DELTA_F_MF_HZ
                && (mid - f64::from(f2.lo)).abs() <= MAX_OFFSET_HZ
            {
                return Some(round_hz(mid));
            }
        }
        at(0)
            .filter(|f1| (f1.hz - f64::from(f1.lo)).abs() <= MAX_OFFSET_HZ)
            .map(|f1| round_hz(f1.hz))
    }
}
