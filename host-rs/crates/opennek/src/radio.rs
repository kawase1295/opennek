//! Real Sigfox RC3 uplinks from the OpenNEK RSSI streamer (firmware-lsm100a, usually behind the ESP32-S3
//! bridge). The streamer sends "I fw=.. f=.. bw=.. sr=.. boost=.." status lines and "R<seq>:<hex>" sample lines.
//!
//! One thread owns the port: it opens it, reads lines with a short timeout and reopens it after an error. A
//! reconnect or stop bumps the generation, which the thread notices between reads.

use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use sigfox::receiver::{ReceiverEvent, SigfoxMessage, SigfoxReceiver};

const RETRY: Duration = Duration::from_secs(2);
/// No samples for this long means the streamer is gone, or never was there
const STALL: Duration = Duration::from_secs(3);
const DEFAULT_RATE: u32 = 2000;
/// Sample rates a streamer can report; anything else is a garbled status line
const RATES: std::ops::RangeInclusive<u32> = 100..=100_000;

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
                Link::new(shared, port, generation).run(&settings);
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
    rx: Option<SigfoxReceiver>,
    last_seq: Option<u16>,
    last_sample: Instant,
}

impl<'a> Link<'a> {
    fn new(shared: &'a Shared, port: Box<dyn LinePort>, generation: u64) -> Self {
        Link {
            shared,
            port,
            generation,
            rx: None,
            last_seq: None,
            last_sample: Instant::now(),
        }
    }

    fn run(mut self, settings: &RadioSettings) {
        let mut result = self.write(&format!("F{}", settings.freq)).and_then(|_| self.write("?"));
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
            if result.is_ok() && watchdog.elapsed() >= Duration::from_secs(1) {
                watchdog = Instant::now();
                result = self.check_stall();
            }
        }
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
        if let Some(status) = line.strip_prefix("I ") {
            self.status_line(status);
        } else if let Some((seq, hex)) = sample_line(line, 'R') {
            self.samples(seq, hex);
        } else if sample_line(line, 'C').is_some() || line.starts_with("D ") {
            // CFO lines and downlink results are used from #33 on
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
        self.shared.update(|s| {
            s.state = RadioState::Connected;
            s.fw = fw;
            s.freq = freq;
            s.rate = Some(rate);
            s.error = None;
        });
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
        self.emit(events, time);
    }

    fn start_receiver(&mut self, rate: u32) {
        self.flush();
        self.rx = Some(SigfoxReceiver::new(rate));
    }

    /// Emits what the receiver still holds (closing, or a new sample rate)
    fn flush(&mut self) {
        if let Some(mut rx) = self.rx.take() {
            let events = rx.flush();
            self.emit(events, rx.time());
        }
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
                    let received_at = wall(m.time);
                    let first_frame_end_at = m.first_frame_end.map(wall);
                    (self.shared.sink)(RadioEvent::Message(RadioMessage {
                        message: m,
                        received_at,
                        first_frame_end_at,
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
