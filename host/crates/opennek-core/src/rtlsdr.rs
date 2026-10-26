//! An RTL-SDR (e.g. RTL-SDR Blog V3) as the receiver, receive only. It is opened as the port "rtlsdr:<index>" and
//! speaks the RSSI streamer's line protocol (see `radio`), so the host treats it like the XIAO kit: a thread reads
//! IQ samples, `sigfox::envelope` turns them into the envelope of the strongest signal in the Sigfox band, and
//! `VirtualStreamer` writes that out as the streamer's status and R lines and answers its commands.
//!
//! The dongle is tuned IF_HZ below the receiver frequency, so that the band stays clear of the DC spike at its
//! centre. Its levels are dBFS, mapped to dBm with a fixed offset that is only roughly right.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use rtl_sdr_rs::{
    AsyncReadEvent, DEFAULT_ASYNC_BUF_NUMBER, DEFAULT_BUF_LENGTH, DeviceDescriptors, DeviceId, RtlSdr, TunerGain,
};
use sigfox::envelope::{Complex32, Envelope, OUT_RATE};

use crate::platform::Os;
use crate::radio::LinePort;
use crate::serial::PortInfo;

/// What the status line gives as the firmware; the host does not ask it to transmit
pub const FW: &str = "rtl-sdr";
const PREFIX: &str = "rtlsdr:";
/// Samples per R line, as the streamer
const LINE_SAMPLES: usize = 64;
/// The frequencies the streamer accepts
const FREQS: std::ops::RangeInclusive<u32> = 150_000_000..=960_000_000;
/// Where the streamer starts too, until the host tunes it
const RC3_UPLINK_HZ: u32 = 923_200_000;
const IQ_RATE: u32 = 1_024_000;
/// The receiver frequency sits this far above the dongle's centre
const IF_HZ: u32 = 300_000;
/// The RC3 uplink band is 192 kHz wide
const HALF_BAND_HZ: f64 = 100_000.0;
/// 29.7 dB: weak devices decode, and one next to the antenna still does though it clips
const GAIN_TENTHS_DB: i32 = 297;
/// dBm of a full-scale signal at that gain, roughly: the noise floor reads about -122 dBm
const FULL_SCALE_DBM: f32 = -50.0;
/// Reads return this often, so that a stop or reconnect is noticed
const READ_TIMEOUT: Duration = Duration::from_millis(100);

pub fn is_rtlsdr(path: &str) -> bool {
    path.starts_with(PREFIX)
}

/// The port that opens the RTL-SDR at `index`
pub fn port_info(index: usize, product: &str, serial: &str) -> PortInfo {
    let label = ["RTL-SDR", product, serial]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    PortInfo {
        path: format!("{PREFIX}{index}"),
        label,
        kit: false,
    }
}

/// The streamer's side of the protocol, from envelope samples; no I/O
pub struct VirtualStreamer {
    freq: u32,
    seq: u16,
    line: Vec<u8>,
}

impl VirtualStreamer {
    pub fn new(freq: u32) -> Self {
        Self {
            freq,
            seq: 0,
            line: Vec::with_capacity(LINE_SAMPLES),
        }
    }

    /// The receiver frequency asked for
    pub fn freq(&self) -> u32 {
        self.freq
    }

    pub fn status(&self) -> String {
        format!("I fw={FW} f={} sr={OUT_RATE} tx=0 hw={FW}", self.freq)
    }

    /// The lines that answer one command line. Settings of the SX126x (W, S, B, P) and C/E have nothing to change
    /// here, and are answered with the status like the streamer does
    pub fn command(&mut self, line: &str) -> Vec<String> {
        let Some(c) = line.chars().next() else { return vec![] };
        match c {
            'T' => return vec!["D off".into()],
            'F' => {
                if let Some(f) = line[1..].trim().parse().ok().filter(|f| FREQS.contains(f)) {
                    self.freq = f;
                }
            }
            'W' | 'S' | 'B' | 'C' | 'P' | 'E' | '?' => {}
            _ => return vec![],
        }
        vec![self.status()]
    }

    /// Envelope samples (dBFS) as R lines, each LINE_SAMPLES long; the rest waits for the next call
    pub fn samples(&mut self, dbfs: &[f32]) -> Vec<String> {
        let mut lines = vec![];
        for &v in dbfs {
            // -2 x dBm, as the streamer sends RSSI
            let raw = (-2.0 * (v + FULL_SCALE_DBM)).round().clamp(0.0, 255.0) as u8;
            self.line.push(raw);
            if self.line.len() == LINE_SAMPLES {
                let hex: String = self.line.iter().map(|b| format!("{b:02X}")).collect();
                lines.push(format!("R{:04X}:{hex}", self.seq));
                self.seq = self.seq.wrapping_add(1);
                self.line.clear();
            }
        }
        lines
    }
}

/// The RTL-SDRs present, as ports
pub fn list_ports() -> Vec<PortInfo> {
    DeviceDescriptors::new()
        .map(|d| d.iter().map(|d| port_info(d.index, &d.product, &d.serial)).collect())
        .unwrap_or_default()
}

fn index_of(path: &str) -> io::Result<usize> {
    path.strip_prefix(PREFIX)
        .and_then(|i| i.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("not an RTL-SDR: {path}")))
}

fn open_error(e: rtl_sdr_rs::error::RtlsdrError) -> io::Error {
    use rtl_sdr_rs::error::{RtlsdrError, UsbErrorKind};
    let kind = match &e {
        RtlsdrError::Usb(u) => Some(u.kind()),
        _ => None,
    };
    let hint = match (kind, Os::current()) {
        (Some(UsbErrorKind::PermissionDenied), Os::Linux) => {
            " — let your user open the RTL-SDR with a udev rule (see the README)"
        }
        (Some(UsbErrorKind::Busy), _) => " — another program is using the RTL-SDR",
        (_, Os::Windows) => " — install the WinUSB driver for the RTL-SDR with Zadig",
        _ => "",
    };
    io::Error::other(format!("RTL-SDR: {e}{hint}"))
}

/// Linux binds the DVB-T driver (dvb_usb_rtl28xxu) to the dongle; let go of it as librtlsdr does
#[cfg(target_os = "linux")]
fn detach_dvb_driver(index: usize) {
    use nusb::MaybeFuture;
    let Ok(known) = DeviceDescriptors::new() else { return };
    let ids: Vec<(u16, u16)> = known.iter().map(|d| (d.vendor_id, d.product_id)).collect();
    let Ok(devices) = nusb::list_devices().wait() else {
        return;
    };
    let mut rtl = devices.filter(|d| ids.contains(&(d.vendor_id(), d.product_id())));
    if let Some(device) = rtl.nth(index).and_then(|d| d.open().wait().ok()) {
        // fails when no driver is bound, which is fine
        let _ = device.detach_kernel_driver(0);
    }
}

#[cfg(not(target_os = "linux"))]
fn detach_dvb_driver(_index: usize) {}

fn open_sdr(index: usize, freq: u32) -> rtl_sdr_rs::error::Result<RtlSdr> {
    detach_dvb_driver(index);
    let mut sdr = RtlSdr::open(DeviceId::Index(index))?;
    sdr.set_sample_rate(IQ_RATE)?;
    sdr.set_center_freq(freq - IF_HZ)?;
    sdr.set_tuner_gain(TunerGain::Manual(GAIN_TENTHS_DB))?;
    sdr.reset_buffer()?;
    Ok(sdr)
}

/// Opens the RTL-SDR of an "rtlsdr:<index>" port, tuned to the default receiver frequency until told otherwise
pub fn open(path: &str) -> io::Result<Box<dyn LinePort>> {
    let index = index_of(path)?;
    let (lines_tx, lines) = mpsc::channel();
    let (commands, commands_rx) = mpsc::channel();
    let (opened_tx, opened) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let thread = std::thread::spawn(move || {
        let streamer = VirtualStreamer::new(RC3_UPLINK_HZ);
        match open_sdr(index, streamer.freq()) {
            Ok(sdr) => {
                let _ = opened_tx.send(Ok(()));
                stream(sdr, streamer, &commands_rx, &lines_tx, &stopped);
            }
            Err(e) => {
                let _ = opened_tx.send(Err(open_error(e)));
            }
        }
    });
    match opened.recv() {
        Ok(Ok(())) => Ok(Box::new(SdrPort {
            lines,
            commands,
            stop,
            thread: Some(thread),
        })),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(io::Error::other("RTL-SDR: the reader stopped")),
    }
}

/// Reads IQ until stopped or the dongle fails, which ends the lines with the error. The reader keeps USB
/// transfers queued while the samples are processed, so that none are lost in between
fn stream(
    sdr: RtlSdr,
    mut streamer: VirtualStreamer,
    commands: &mpsc::Receiver<String>,
    lines: &mpsc::Sender<io::Result<String>>,
    stop: &AtomicBool,
) {
    let failed = |e: String| {
        let _ = lines.send(Err(io::Error::other(format!("RTL-SDR: {e}"))));
    };
    let reader = match sdr.into_async_reader(DEFAULT_ASYNC_BUF_NUMBER, DEFAULT_BUF_LENGTH) {
        Ok(r) => r,
        Err(e) => return failed(e.to_string()),
    };
    let control = reader.control_handle();
    let band = (f64::from(IF_HZ) - HALF_BAND_HZ, f64::from(IF_HZ) + HALF_BAND_HZ);
    let mut envelope = Envelope::new(IQ_RATE, band);
    let send = |l: String| lines.send(Ok(l)).is_ok();
    if !send(streamer.status()) {
        return;
    }
    while !stop.load(Ordering::SeqCst) {
        while let Ok(command) = commands.try_recv() {
            let freq = streamer.freq();
            for reply in streamer.command(&command) {
                send(reply);
            }
            if streamer.freq() != freq
                && let Err(e) = control.set_center_freq(streamer.freq() - IF_HZ)
            {
                return failed(e.to_string());
            }
        }
        let data = match reader.recv() {
            Some(Ok(AsyncReadEvent::Samples { data, .. })) => data,
            Some(Ok(_)) => continue,
            Some(Err(e)) => return failed(e.to_string()),
            None => return failed("the stream stopped".into()),
        };
        let iq: Vec<Complex32> = data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[i, q]| Complex32::new((f32::from(i) - 127.5) / 128.0, (f32::from(q) - 127.5) / 128.0))
            .collect();
        for line in streamer.samples(&envelope.push(&iq)) {
            if !send(line) {
                return;
            }
        }
    }
}

struct SdrPort {
    lines: mpsc::Receiver<io::Result<String>>,
    commands: mpsc::Sender<String>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl LinePort for SdrPort {
    fn read_line(&mut self) -> io::Result<Option<String>> {
        match self.lines.recv_timeout(READ_TIMEOUT) {
            Ok(line) => line.map(Some),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "RTL-SDR: the reader stopped",
            )),
        }
    }

    fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.commands
            .send(line.trim().to_string())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "RTL-SDR: the reader stopped"))
    }
}

impl Drop for SdrPort {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
