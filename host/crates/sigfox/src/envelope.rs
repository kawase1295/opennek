//! The envelope of a Sigfox uplink from complex baseband (IQ) samples, such as an RTL-SDR's, for the envelope
//! demodulator (`demod`). A frame is about 100 Hz wide and may sit anywhere in a band of ~200 kHz, a different
//! place for each repetition, so the band is cut into 1 kHz bins (Hann-windowed FFTs, half overlapping, one per
//! output sample) and each output sample is the power around the bin that is strongest over 20 ms on each side.
//! That bin holds still through the dips of a frame, so they show as they do in a receiver's RSSI.

use std::collections::VecDeque;
use std::sync::Arc;

use rustfft::{Fft, FftPlanner};

pub use rustfft::num_complex::Complex32;

/// Output samples per second, the rate `demod` expects (20 per bit)
pub const OUT_RATE: u32 = 2000;
/// Spectra on each side of an output sample that pick its bin; also the output's delay
const SELECT: usize = 40;

pub struct Envelope {
    /// FFT length: two hops, so the bins are OUT_RATE / 2 wide
    n: usize,
    hop: usize,
    bin_hz: f64,
    /// Band edges in bins, from the IQ centre frequency
    low: i64,
    high: i64,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    /// Samples not yet in a spectrum
    pending: Vec<Complex32>,
    /// Power spectra around the output sample, the newest last
    spectra: VecDeque<Vec<f32>>,
    /// Their sum per bin
    sum: Vec<f64>,
    /// The power of a full-scale tone in three bins
    full_scale: f64,
}

impl Envelope {
    /// `rate`: IQ samples per second, a multiple of 2 * OUT_RATE. `band`: its edges in Hz from the centre frequency
    pub fn new(rate: u32, band: (f64, f64)) -> Self {
        assert!(
            rate.is_multiple_of(2 * OUT_RATE),
            "an IQ rate that is not a multiple of {}",
            2 * OUT_RATE
        );
        let hop = (rate / OUT_RATE) as usize;
        let n = 2 * hop;
        // periodic Hann
        let window = (0..n)
            .map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()) as f32)
            .collect();
        let mut env = Self {
            n,
            hop,
            bin_hz: f64::from(rate) / n as f64,
            low: 0,
            high: 0,
            fft: FftPlanner::new().plan_fft_forward(n),
            window,
            pending: Vec::with_capacity(2 * n),
            spectra: VecDeque::with_capacity(2 * SELECT + 1),
            sum: vec![0.0; n],
            // Parseval: the Hann-windowed tone's power over all bins, which three bins hold within 2 %
            full_scale: 3.0 * (n * n) as f64 / 8.0,
        };
        env.set_band(band.0, band.1);
        env
    }

    /// Moves the band, e.g. when the receiver is asked for another frequency without retuning the hardware
    pub fn set_band(&mut self, low_hz: f64, high_hz: f64) {
        self.low = (low_hz / self.bin_hz).ceil() as i64;
        self.high = (high_hz / self.bin_hz).floor() as i64;
    }

    /// Envelope samples in dBFS (0 for a full-scale tone), SELECT samples behind the input
    pub fn push(&mut self, iq: &[Complex32]) -> Vec<f32> {
        self.pending.extend_from_slice(iq);
        let mut out = Vec::with_capacity(self.pending.len() / self.hop);
        let mut from = 0;
        while self.pending.len() - from >= self.n {
            let spectrum = self.spectrum(&self.pending[from..from + self.n]);
            from += self.hop;
            if let Some(v) = self.add(spectrum) {
                out.push(v);
            }
        }
        self.pending.drain(..from);
        out
    }

    fn spectrum(&self, samples: &[Complex32]) -> Vec<f32> {
        let mut buf: Vec<Complex32> = samples.iter().zip(&self.window).map(|(&s, &w)| s * w).collect();
        self.fft.process(&mut buf);
        buf.iter().map(|c| c.norm_sqr()).collect()
    }

    fn bin(&self, k: i64) -> usize {
        k.rem_euclid(self.n as i64) as usize
    }

    /// Takes the newest spectrum; the output sample for the one SELECT spectra back, once there is one
    fn add(&mut self, spectrum: Vec<f32>) -> Option<f32> {
        for (s, &p) in self.sum.iter_mut().zip(&spectrum) {
            *s += f64::from(p);
        }
        self.spectra.push_back(spectrum);
        if self.spectra.len() > 2 * SELECT + 1
            && let Some(old) = self.spectra.pop_front()
        {
            for (s, &p) in self.sum.iter_mut().zip(&old) {
                *s -= f64::from(p);
            }
        }
        let centre = self.spectra.len().checked_sub(SELECT + 1)?;
        let best = (self.low..=self.high).max_by(|&a, &b| self.sum[self.bin(a)].total_cmp(&self.sum[self.bin(b)]))?;
        let p = &self.spectra[centre];
        let power: f64 = (best - 1..=best + 1).map(|k| f64::from(p[self.bin(k)])).sum();
        Some((10.0 * (power / self.full_scale).max(1e-30).log10()) as f32)
    }
}
