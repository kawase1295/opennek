//! Real Sigfox RC3 uplinks from the OpenNEK RSSI streamer (firmware-lsm100a, usually behind the ESP32-S3
//! bridge). The streamer sends "I fw=.. f=.. bw=.. sr=.. boost=.." status lines, "R<seq>:<hex>" sample lines and,
//! from fw 0.2.0, "C<seq>:<hex>" CFO lines that give the frequency of each frame (sigfox::frequency). From fw 0.3.0
//! it also transmits the downlinks: "T<hz>,<dBm>,<hex>" answered by "D <result>", allowed with "E1" only around
//! each transmission. The firmware enforces the channel, the power cap, carrier sense and the transmit-time limits.
//!
//! One thread owns the port: it opens it, reads lines with a short timeout and reopens it after an error. A
//! reconnect or stop bumps the generation, which the thread notices between reads. Other threads write to the port
//! through a channel the thread drains between reads.

use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use sigfox::frame::DeviceId;
use sigfox::frequency::{FrequencyTracker, STREAMER_BIT_RATE, cfo_step_hz};
use sigfox::receiver::{ReceiverEvent, SigfoxMessage, SigfoxReceiver};

const RETRY: Duration = Duration::from_secs(2);
/// No samples for this long means the streamer is gone, or never was there
const STALL: Duration = Duration::from_secs(3);
const DEFAULT_RATE: u32 = 2000;
/// Sample rates a streamer can report; anything else is a garbled status line
const RATES: std::ops::RangeInclusive<u32> = 100..=100_000;
/// The streamer's receive bandwidth (RX_BW code), which sets the Hz per CFO step
const STREAMER_BW: u8 = 0x0a;
/// The first firmware-lsm100a that transmits
const TX_FW: [u32; 3] = [0, 3, 0];
/// The firmware's maximum
const TX_DBM: i32 = 10;
/// Carrier sense and 280 ms on air
const TX_REPLY: Duration = Duration::from_secs(2);
/// After "busy": the channel was in use
const TX_RETRY: Duration = Duration::from_millis(200);

/// A line-oriented connection to the receiver. `read_line` returns None when nothing arrived for a while.
pub trait LinePort: Send {
    fn read_line(&mut self) -> io::Result<Option<String>>;
    fn write_line(&mut self, line: &str) -> io::Result<()>;
}

pub trait Opener: Send + Sync {
    fn open(&self, path: &str) -> io::Result<Box<dyn LinePort>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadioSettings {
    /// Empty disables the receiver
    pub port: String,
    pub freq: u32,
    /// Tune to frame 1 of each message, so that its frequency can be measured for a downlink, then back to freq.
    /// It costs reception of other devices' first frames, so it is on only while downlinks are
    pub follow: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RadioState {
    Off,
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioStatus {
    pub state: RadioState,
    pub port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freq: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Frames with a valid CRC
    pub frames: u64,
    pub bad_frames: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_frame_at: Option<i64>,
    pub lost_lines: u64,
}

impl Default for RadioStatus {
    fn default() -> Self {
        Self {
            state: RadioState::Off,
            port: None,
            fw: None,
            freq: None,
            rate: None,
            error: None,
            frames: 0,
            bad_frames: 0,
            last_frame_at: None,
            lost_lines: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadioMessage {
    pub message: SigfoxMessage,
    /// Wall-clock ms of the first frame
    pub received_at: i64,
    /// Wall-clock ms of the end of frame 1, which starts the device's downlink timer
    pub first_frame_end_at: Option<i64>,
    /// Carrier frequency of frame 1, which the downlink frequency is derived from. None when it could not be measured
    /// closely enough: streamer before 0.2.0, frequency following off, or frames missed
    pub uplink_freq_hz: Option<u32>,
}

/// One downlink frame, to start before `deadline` (wall-clock ms)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownlinkTx {
    pub device_id: DeviceId,
    pub counter: u16,
    pub freq_hz: u32,
    pub frame: [u8; 15],
    pub deadline: i64,
}

/// Sends Sigfox downlinks. The transmitter owns the regulatory side and answers false when it did not transmit,
/// e.g. after the deadline. It blocks until then
pub trait Transmitter: Send + Sync {
    fn transmit(&self, tx: &DownlinkTx) -> bool;
}

/// A line for the port owner to write: with where to say it was written, or where to send the "D" reply to it
enum Command {
    Write(String, Option<mpsc::Sender<()>>),
    Ask(String, mpsc::Sender<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum RadioEvent {
    Status(RadioStatus),
    Message(RadioMessage),
    Log(String),
}

pub type SettingsFn = Box<dyn Fn() -> RadioSettings + Send + Sync>;
pub type Sink = Box<dyn Fn(RadioEvent) + Send + Sync>;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

struct Shared {
    settings: SettingsFn,
    opener: Box<dyn Opener>,
    sink: Sink,
    status: Mutex<RadioStatus>,
    generation: AtomicU64,
    stopped: AtomicBool,
    /// To the thread that owns the open port
    commands: Mutex<Option<mpsc::Sender<Command>>>,
    tx_busy: AtomicBool,
    /// RadioSettings::follow, changed without reconnecting
    follow: AtomicBool,
}

impl Shared {
    fn status(&self) -> MutexGuard<'_, RadioStatus> {
        self.status.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn update(&self, patch: impl FnOnce(&mut RadioStatus)) {
        let status = {
            let mut s = self.status();
            patch(&mut s);
            s.clone()
        };
        (self.sink)(RadioEvent::Status(status));
    }

    fn log(&self, line: String) {
        (self.sink)(RadioEvent::Log(line));
    }

    fn current(&self, generation: u64) -> bool {
        !self.stopped.load(Ordering::SeqCst) && self.generation.load(Ordering::SeqCst) == generation
    }

    /// Sleeps up to `d`, returning early on a reconnect or stop
    fn wait(&self, generation: u64, d: Duration) {
        let until = Instant::now().checked_add(d);
        while self.current(generation) && until.is_none_or(|u| Instant::now() < u) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

pub struct Radio {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Radio {
    pub fn new(settings: SettingsFn, opener: Box<dyn Opener>, sink: Sink) -> Radio {
        let shared = Shared {
            settings,
            opener,
            sink,
            status: Mutex::new(RadioStatus::default()),
            generation: AtomicU64::new(0),
            stopped: AtomicBool::new(true),
            commands: Mutex::new(None),
            tx_busy: AtomicBool::new(false),
            follow: AtomicBool::new(false),
        };
        Radio {
            shared: Arc::new(shared),
            thread: Mutex::new(None),
        }
    }

    pub fn status(&self) -> RadioStatus {
        self.shared.status().clone()
    }

    pub fn start(&self) {
        let mut thread = self.thread.lock().unwrap_or_else(|e| e.into_inner());
        if thread.is_some() {
            return;
        }
        self.shared.stopped.store(false, Ordering::SeqCst);
        let shared = self.shared.clone();
        *thread = Some(std::thread::spawn(move || run(&shared)));
    }

    pub fn stop(&self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = t.join();
        }
    }

    /// Reopens the port with the current settings
    pub fn reconnect(&self) {
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Starts or stops following frame 1 (see RadioSettings::follow)
    pub fn set_follow(&self, follow: bool) {
        self.shared.follow.store(follow, Ordering::SeqCst);
    }

    fn tx_failed(&self, why: &str) -> bool {
        self.shared.log(format!("sigfox: TX not sent: {why}"));
        false
    }

    // Retried while the channel is busy, until the deadline
    fn send_frame(&self, commands: &mpsc::Sender<Command>, tx: &DownlinkTx) -> bool {
        let hex: String = tx.frame.iter().map(|b| format!("{b:02X}")).collect();
        loop {
            if now_ms() > tx.deadline {
                return self.tx_failed("the device's receive window has closed");
            }
            let (reply, answer) = mpsc::channel();
            if commands
                .send(Command::Ask(format!("T{},{TX_DBM},{hex}", tx.freq_hz), reply))
                .is_err()
            {
                return self.tx_failed("receiver not connected");
            }
            let result = answer.recv_timeout(TX_REPLY).ok();
            self.shared.log(format!(
                "sigfox: TX {:.6} MHz {TX_DBM} dBm: {}",
                f64::from(tx.freq_hz) / 1e6,
                result.as_deref().unwrap_or("no reply")
            ));
            match result.as_deref() {
                Some(r) if r.starts_with("ok") => return true,
                Some(r) if r.starts_with("busy") => std::thread::sleep(TX_RETRY),
                _ => return false,
            }
        }
    }
}

// A part that is not a number ("0-rc1") is not at least anything, as NaN in the TypeScript host
fn at_least(fw: Option<&str>, min: [u32; 3]) -> bool {
    let v: Vec<Option<u32>> = fw
        .unwrap_or_default()
        .split('.')
        .map(|p| if p.is_empty() { Some(0) } else { p.parse().ok() })
        .collect();
    for (i, m) in min.iter().enumerate() {
        match v.get(i).copied().unwrap_or(Some(0)) {
            Some(n) if n == *m => continue,
            Some(n) => return n > *m,
            None => return false,
        }
    }
    true
}

impl Transmitter for Radio {
    fn transmit(&self, tx: &DownlinkTx) -> bool {
        let status = self.status();
        let commands = self.shared.commands.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(commands) = commands.filter(|_| status.state == RadioState::Connected) else {
            return self.tx_failed("receiver not connected");
        };
        if !at_least(status.fw.as_deref(), TX_FW) {
            let fw = status.fw.as_deref().unwrap_or("?");
            return self.tx_failed(&format!("firmware-lsm100a 0.3.0 or later needed (fw={fw})"));
        }
        if self.shared.tx_busy.swap(true, Ordering::SeqCst) {
            return self.tx_failed("another downlink is being sent");
        }
        let _ = commands.send(Command::Write("E1".into(), None));
        let sent = self.send_frame(&commands, tx);
        // TX stays allowed only while needed: wait until E0 is out
        let (written, done) = mpsc::channel();
        if commands.send(Command::Write("E0".into(), Some(written))).is_ok() {
            let _ = done.recv_timeout(TX_REPLY);
        }
        self.shared.tx_busy.store(false, Ordering::SeqCst);
        sent
    }
}

fn run(shared: &Shared) {
    while !shared.stopped.load(Ordering::SeqCst) {
        let generation = shared.generation.load(Ordering::SeqCst);
        let settings = (shared.settings)();
        if settings.port.is_empty() {
            shared.update(|s| {
                s.state = RadioState::Off;
                s.port = None;
                s.error = None;
            });
            shared.wait(generation, Duration::MAX);
            continue;
        }
        shared.update(|s| {
            s.state = RadioState::Connecting;
            s.port = Some(settings.port.clone());
            s.error = None;
        });
        match shared.opener.open(&settings.port) {
            Ok(port) => {
                shared.log(format!("sigfox: opened {}", settings.port));
                Link::new(shared, port, generation, settings.clone()).run();
            }
            Err(e) => {
                shared.update(|s| {
                    s.state = RadioState::Disconnected;
                    s.error = Some(e.to_string());
                });
                shared.wait(generation, RETRY);
            }
        }
    }
}

/// One open port
struct Link<'a> {
    shared: &'a Shared,
    port: Box<dyn LinePort>,
    generation: u64,
    settings: RadioSettings,
    commands: mpsc::Receiver<Command>,
    /// Waiting for the "D" reply of a T command
    reply: Option<mpsc::Sender<String>>,
    rx: Option<SigfoxReceiver>,
    tracker: Option<FrequencyTracker>,
    last_seq: Option<u16>,
    last_sample: Instant,
}

impl<'a> Link<'a> {
    fn new(shared: &'a Shared, port: Box<dyn LinePort>, generation: u64, settings: RadioSettings) -> Self {
        let (sender, commands) = mpsc::channel();
        *shared.commands.lock().unwrap_or_else(|e| e.into_inner()) = Some(sender);
        shared.follow.store(settings.follow, Ordering::SeqCst);
        Link {
            shared,
            port,
            generation,
            settings,
            commands,
            reply: None,
            rx: None,
            tracker: None,
            last_seq: None,
            last_sample: Instant::now(),
        }
    }

    fn run(mut self) {
        let tune = format!("F{}", self.settings.freq);
        // C1: CFO lines (fw 0.2.0); older streamers ignore it
        let mut result = self
            .write(&tune)
            .and_then(|_| self.write("C1"))
            .and_then(|_| self.write("?"));
        let mut watchdog = Instant::now();
        while result.is_ok() && self.shared.current(self.generation) {
            result = match self.port.read_line() {
                Ok(Some(line)) => {
                    self.handle_line(&line);
                    Ok(())
                }
                Ok(None) => Ok(()),
                Err(e) => Err(e),
            };
            if result.is_ok() {
                result = self.run_commands();
            }
            if result.is_ok() && watchdog.elapsed() >= Duration::from_secs(1) {
                watchdog = Instant::now();
                result = self.check_stall();
            }
        }
        *self.shared.commands.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.flush();
        match result {
            Ok(()) => self.shared.update(|s| {
                if matches!(s.state, RadioState::Connected | RadioState::Connecting) {
                    s.state = RadioState::Disconnected;
                }
            }),
            Err(e) => {
                self.shared.log(format!("sigfox: {e}"));
                self.shared.update(|s| {
                    s.state = RadioState::Disconnected;
                    s.error = Some("Lost the connection to the receiver".into());
                });
                self.shared.wait(self.generation, RETRY);
            }
        }
    }

    fn write(&mut self, line: &str) -> io::Result<()> {
        self.port.write_line(line)
    }

    fn run_commands(&mut self) -> io::Result<()> {
        while let Ok(command) = self.commands.try_recv() {
            match command {
                Command::Write(line, written) => {
                    self.write(&line)?;
                    if let Some(written) = written {
                        let _ = written.send(());
                    }
                }
                Command::Ask(line, reply) => {
                    self.reply = Some(reply);
                    self.write(&line)?;
                }
            }
        }
        Ok(())
    }

    fn check_stall(&mut self) -> io::Result<()> {
        if self.last_sample.elapsed() < STALL {
            return Ok(());
        }
        self.shared.update(|s| {
            s.state = RadioState::Connecting;
            s.error = Some("No reply from the RSSI streamer (flash firmware-lsm100a and connect at 115200 bps)".into());
        });
        self.write("?")
    }

    fn handle_line(&mut self, raw: &str) {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(result) = line.strip_prefix("D ") {
            if let Some(reply) = self.reply.take() {
                let _ = reply.send(result.trim().to_string());
            }
        } else if let Some(status) = line.strip_prefix("I ") {
            self.status_line(status);
        } else if let Some((seq, hex)) = sample_line(line, 'R') {
            self.samples(seq, hex);
        } else if line.starts_with('C') && line.get(5..6) == Some(":") {
            // a garbled sequence number matches no R line
            if let Some((seq, hex)) = sample_line(line, 'C') {
                self.cfo(seq, hex);
            }
        } else if !line.trim().is_empty() {
            self.shared.log(format!("sigfox: {}", line.trim()));
        }
    }

    fn status_line(&mut self, line: &str) {
        let field = |k: &str| {
            line.split_whitespace()
                .find_map(|f| f.strip_prefix(k).and_then(|v| v.strip_prefix('=')))
                .map(str::to_string)
        };
        let current = self.rx.as_ref().map_or(DEFAULT_RATE, SigfoxReceiver::rate);
        let rate = field("sr")
            .and_then(|v| v.parse().ok())
            .filter(|r| RATES.contains(r))
            .unwrap_or(current);
        if self.rx.as_ref().is_none_or(|rx| rx.rate() != rate) {
            self.start_receiver(rate);
        }
        let fw = field("fw");
        let freq = field("f").and_then(|v| v.parse().ok()).filter(|&f: &u32| f != 0);
        if let (Some(f), Some(tracker)) = (freq, self.tracker.as_mut()) {
            tracker.tuned(f);
        }
        self.shared.update(|s| {
            s.state = RadioState::Connected;
            s.fw = fw;
            s.freq = freq;
            s.rate = Some(rate);
            s.error = None;
        });
    }

    // CFO values for the R line with the same seq, which came just before
    fn cfo(&mut self, seq: u16, hex: &str) {
        if self.last_seq != Some(seq) {
            return;
        }
        let raw: Vec<u16> = (0..hex.len() / 3)
            .map(|i| {
                hex.get(i * 3..i * 3 + 3)
                    .and_then(|v| u16::from_str_radix(v, 16).ok())
                    .unwrap_or(0)
            })
            .collect();
        if let Some(tracker) = self.tracker.as_mut() {
            tracker.cfo(&raw);
        }
    }

    fn samples(&mut self, seq: u16, hex: &str) {
        let samples = parse_hex(hex);
        if let Some(last) = self.last_seq
            && seq != last.wrapping_add(1)
        {
            let lost = u64::from(seq.wrapping_sub(last).wrapping_sub(1));
            self.shared.update(|s| s.lost_lines += lost);
        }
        self.last_seq = Some(seq);
        self.last_sample = Instant::now();
        if self.rx.is_none() {
            self.start_receiver(DEFAULT_RATE);
        }
        if self.shared.status().state != RadioState::Connected {
            let rate = self.rx.as_ref().map(SigfoxReceiver::rate);
            self.shared.update(|s| {
                s.state = RadioState::Connected;
                s.rate = rate;
                s.error = None;
            });
        }
        let Some(rx) = self.rx.as_mut() else { return };
        let events = rx.push_raw(&samples);
        let time = rx.time();
        let dbm: Vec<f32> = samples.iter().map(|&v| -f32::from(v) / 2.0).collect();
        let tuning = self.tracker.as_mut().and_then(|tracker| {
            tracker.samples(&dbm);
            if self.shared.follow.load(Ordering::SeqCst) {
                tracker.next_tuning()
            } else {
                None
            }
        });
        self.emit(events, time);
        if let Some(freq) = tuning {
            self.shared
                .log(format!("sigfox: tuning to {:.6} MHz", f64::from(freq) / 1e6));
            // a failed write shows up as a read error next
            let _ = self.write(&format!("F{freq}"));
        }
    }

    fn start_receiver(&mut self, rate: u32) {
        self.flush();
        self.rx = Some(SigfoxReceiver::new(rate));
        let step = cfo_step_hz(STREAMER_BW, STREAMER_BIT_RATE);
        self.tracker = Some(FrequencyTracker::new(rate, self.settings.freq, step));
    }

    /// Emits what the receiver still holds (closing, or a new sample rate)
    fn flush(&mut self) {
        if let Some(mut rx) = self.rx.take() {
            let events = rx.flush();
            self.emit(events, rx.time());
        }
    }

    /// The receiver frequency and CFO of each frame, to check the measurement
    fn log_frame1(&self, m: &SigfoxMessage, uplink_freq_hz: Option<u32>) {
        let Some(tracker) = &self.tracker else { return };
        let readings: Vec<String> = m
            .frames
            .iter()
            .map(|f| match tracker.frame_frequency(f) {
                Some(r) => {
                    let cfo = (r.hz - f64::from(r.lo) + 0.5).floor() as i64;
                    format!(
                        "{}: {:.6} {}{cfo} Hz",
                        f.rank + 1,
                        f64::from(r.lo) / 1e6,
                        if cfo >= 0 { "+" } else { "" }
                    )
                }
                None => format!("{}: -", f.rank + 1),
            })
            .collect();
        let at = uplink_freq_hz.map_or("not measured".into(), |f| format!("at {:.6} MHz", f64::from(f) / 1e6));
        self.shared.log(format!(
            "sigfox: frame 1 of {} seq={} {at} ({})",
            m.device_id,
            m.counter,
            readings.join(", ")
        ));
    }

    /// `time`: the receiver's stream time now, which maps stream times to the wall clock
    fn emit(&self, events: Vec<ReceiverEvent>, time: f64) {
        let now = now_ms();
        let wall = |t: f64| now - ((time - t) * 1000.0).round() as i64;
        for e in events {
            match e {
                ReceiverEvent::Frame(d) => {
                    let f = &d.frame;
                    self.shared.update(|s| {
                        if f.crc_ok {
                            s.frames += 1;
                            s.last_frame_at = Some(now);
                        } else {
                            s.bad_frames += 1;
                        }
                    });
                    let crc = match (&f.crc_ok, &f.repaired) {
                        (true, Some(how)) => format!("CRC ok ({how})"),
                        (true, None) => "CRC ok".into(),
                        (false, _) => "CRC error".into(),
                    };
                    self.shared.log(format!(
                        "sigfox: frame {}/3 {} seq={} {crc} rssi={:.1}",
                        f.rank + 1,
                        f.device_id,
                        f.counter,
                        d.rssi
                    ));
                }
                ReceiverEvent::Message(m) => {
                    let uplink_freq_hz = self.tracker.as_ref().and_then(|t| t.frame1(&m.frames, m.downlink));
                    if m.downlink {
                        self.log_frame1(&m, uplink_freq_hz);
                    }
                    let received_at = wall(m.time);
                    let first_frame_end_at = m.first_frame_end.map(wall);
                    (self.shared.sink)(RadioEvent::Message(RadioMessage {
                        message: m,
                        received_at,
                        first_frame_end_at,
                        uplink_freq_hz,
                    }));
                }
            }
        }
    }
}

/// "R<seq>:<hex>": the sequence number and the hex after the colon
fn sample_line(line: &str, kind: char) -> Option<(u16, &str)> {
    let rest = line.strip_prefix(kind)?;
    let (seq, hex) = (rest.get(..4)?, rest.get(4..)?.strip_prefix(':')?);
    Some((u16::from_str_radix(seq, 16).ok()?, hex))
}

// Like the TypeScript host: a pair that is not hex reads as 0, so the line keeps its length and the stream its time
fn parse_hex(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|i| {
            hex.get(i * 2..i * 2 + 2)
                .and_then(|b| u8::from_str_radix(b, 16).ok())
                .unwrap_or(0)
        })
        .collect()
}
