//! Streaming Sigfox receiver: takes RSSI samples as they arrive, decodes complete frames and merges the up
//! to three repetitions of a message into one.

use crate::demod::{BurstOptions, DecodedFrame, decode_burst, find_bursts, noise_floor};
use crate::frame::{DeviceId, UplinkFields};
use crate::frequency::FrameSpan;

#[derive(Clone, Debug, PartialEq)]
pub struct SigfoxMessage {
    pub device_id: DeviceId,
    pub counter: u16,
    pub payload: Vec<u8>,
    pub bit: Option<bool>,
    pub downlink: bool,
    /// Keep-alive or downlink confirmation
    pub control: bool,
    pub auth: Vec<u8>,
    /// Stream time of the first frame received, in seconds
    pub time: f64,
    /// Stream time of the end of frame 1 (rank 0), which starts the device's downlink timer
    pub first_frame_end: Option<f64>,
    /// Best RSSI of the received frames (dBm)
    pub rssi: f32,
    /// Which repetitions (0..2) were received with a valid CRC
    pub ranks: Vec<usize>,
    /// When each of them was on air
    pub frames: Vec<FrameSpan>,
    pub clock_error_ppm: i32,
}

impl SigfoxMessage {
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

#[derive(Clone, Debug, PartialEq)]
pub enum ReceiverEvent {
    /// Every decoded frame, valid CRC or not
    Frame(DecodedFrame),
    /// A message once its repetitions are in
    Message(SigfoxMessage),
}

const BUFFER_S: f64 = 12.0;
const STEP_S: f64 = 0.5;
/// A burst counts as finished once it ended this long before the newest sample
const TAIL_S: f64 = 0.3;
// Repetitions start ~3.7-4 s apart and a frame is only decoded ~2 s after it starts, so wait this long
// after the latest repetition before calling a message complete
const QUIET_S: f64 = 10.0;
/// From the end of one repetition to the start of the next; the same for 1- and 5-byte payloads (AX-SFJK, RC3)
pub const FRAME_GAP_S: f64 = 2.43;
/// The detected start of a burst moves a little between scans
const SAME_BURST_S: f64 = 0.3;

struct Pending {
    message: SigfoxMessage,
    /// Stream time of the latest repetition
    last: f64,
}

pub struct SigfoxReceiver {
    rate: u32,
    buf: Vec<f32>,
    /// Samples in buf
    len: usize,
    /// Absolute index of buf[0]
    base: usize,
    since_scan: usize,
    /// Absolute start samples of bursts already decoded
    seen: Vec<usize>,
    /// In arrival order, so that messages complete in the order they started
    pending: Vec<Pending>,
}

impl SigfoxReceiver {
    pub fn new(rate: u32) -> Self {
        Self {
            rate,
            buf: vec![0.0; (BUFFER_S * f64::from(rate)).round() as usize],
            len: 0,
            base: 0,
            since_scan: 0,
            seen: vec![],
            pending: vec![],
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Stream time of the newest sample, in seconds
    pub fn time(&self) -> f64 {
        (self.base + self.len) as f64 / f64::from(self.rate)
    }

    /// Raw streamer values (dBm = -value / 2)
    pub fn push_raw(&mut self, raw: &[u8]) -> Vec<ReceiverEvent> {
        let dbm: Vec<f32> = raw.iter().map(|&r| -f32::from(r) / 2.0).collect();
        self.push(&dbm)
    }

    pub fn push(&mut self, dbm: &[f32]) -> Vec<ReceiverEvent> {
        let mut events = vec![];
        let step = STEP_S * f64::from(self.rate);
        let mut off = 0;
        while off < dbm.len() {
            if self.len == self.buf.len() {
                // keep the newest half
                let keep = self.buf.len() / 2;
                self.buf.copy_within(self.len - keep..self.len, 0);
                self.base += self.len - keep;
                self.len = keep;
            }
            let n = (dbm.len() - off).min(self.buf.len() - self.len);
            self.buf[self.len..self.len + n].copy_from_slice(&dbm[off..off + n]);
            self.len += n;
            off += n;
            self.since_scan += n;
            if self.since_scan as f64 >= step {
                self.since_scan = 0;
                self.scan(false, &mut events);
            }
        }
        self.expire(false, &mut events);
        events
    }

    /// Emits everything still pending (end of a capture or of a connection)
    pub fn flush(&mut self) -> Vec<ReceiverEvent> {
        let mut events = vec![];
        self.scan(true, &mut events);
        self.expire(true, &mut events);
        events
    }

    fn scan(&mut self, last: bool, events: &mut Vec<ReceiverEvent>) {
        let rate = f64::from(self.rate);
        let view = &self.buf[..self.len];
        let tail = if last { 0 } else { (TAIL_S * rate).round() as usize };
        let t0 = self.base as f64 / rate;
        for b in find_bursts(view, self.rate, &BurstOptions::default(), noise_floor(view)) {
            // Only bursts that have ended; ones touching the start of the buffer were decoded earlier
            if b.end + tail > self.len || b.start == 0 {
                continue;
            }
            let start = self.base + b.start;
            if self
                .seen
                .iter()
                .any(|&s| (s.abs_diff(start) as f64) < SAME_BURST_S * rate)
            {
                continue;
            }
            self.seen.push(start);
            let Some(frame) = decode_burst(view, self.rate, &b, t0) else {
                continue;
            };
            if frame.frame.crc_ok {
                Self::merge(&mut self.pending, &frame);
            }
            events.push(ReceiverEvent::Frame(frame));
        }
        let oldest = self.base.saturating_sub(self.rate as usize);
        self.seen.retain(|&s| s >= oldest);
    }

    fn merge(pending: &mut Vec<Pending>, d: &DecodedFrame) {
        let f = &d.frame;
        let span = FrameSpan {
            rank: f.rank,
            time: d.time,
            end: d.end,
        };
        if let Some(p) = pending
            .iter_mut()
            .find(|p| p.message.device_id == f.device_id && p.message.counter == f.counter)
        {
            let m = &mut p.message;
            if !m.ranks.contains(&f.rank) {
                m.ranks.push(f.rank);
                m.frames.push(span);
            }
            m.rssi = m.rssi.max(d.rssi);
            p.last = d.time;
            if f.rank == 0 {
                m.first_frame_end = Some(d.end);
            }
            return;
        }
        pending.push(Pending {
            message: SigfoxMessage {
                device_id: f.device_id,
                counter: f.counter,
                payload: f.payload.clone(),
                bit: f.bit,
                downlink: f.downlink,
                control: f.control,
                auth: f.auth.clone(),
                time: d.time,
                first_frame_end: (f.rank == 0).then_some(d.end),
                rssi: d.rssi,
                ranks: vec![f.rank],
                frames: vec![span],
                clock_error_ppm: d.clock_error_ppm,
            },
            last: d.time,
        });
    }

    fn expire(&mut self, all: bool, events: &mut Vec<ReceiverEvent>) {
        let now = self.time();
        let (done, waiting) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|p| all || p.message.ranks.len() == 3 || now - p.last > QUIET_S);
        self.pending = waiting;
        for Pending { mut message, .. } in done {
            message.ranks.sort_unstable();
            message.first_frame_end = message
                .first_frame_end
                .or_else(|| estimate_first_frame_end(&message.frames));
            events.push(ReceiverEvent::Message(message));
        }
    }
}

/// When frame 1 was lost (often when the receiver retuned during it), its end from a later repetition: all three
/// last as long and are FRAME_GAP_S apart. The device starts its downlink timer there.
pub fn estimate_first_frame_end(frames: &[FrameSpan]) -> Option<f64> {
    let f = frames.iter().filter(|x| x.rank > 0).min_by_key(|x| x.rank)?;
    Some(f.end - f.rank as f64 * (f.end - f.time + FRAME_GAP_S))
}
