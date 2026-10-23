#![allow(dead_code)]

use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use opennek::radio::{LinePort, Opener};

pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../fixtures")
        .join(name)
}

// The streamer's status line and its sample lines for a recording, 64 samples per line
pub fn streamer_lines(raw: &[u8]) -> Vec<String> {
    let mut lines = vec!["I fw=0.3.0 f=923200000 bw=10 sr=2000 boost=1 cfo=0 pd=4".to_string()];
    for (seq, chunk) in raw.chunks(64).enumerate() {
        let hex: String = chunk.iter().map(|b| format!("{b:02X}")).collect();
        lines.push(format!("R{:04X}:{hex}", seq & 0xffff));
    }
    lines
}

/// A receiver that sends the given lines, then stays quiet; it records what the host wrote
#[derive(Clone, Default)]
pub struct FakeStreamer {
    pub lines: Arc<Mutex<VecDeque<String>>>,
    pub written: Arc<Mutex<Vec<String>>>,
    pub opened: Arc<Mutex<Vec<String>>>,
    /// Paths that fail to open, with the error
    pub broken: Arc<Mutex<Vec<String>>>,
}

impl FakeStreamer {
    pub fn with_lines(lines: Vec<String>) -> Self {
        let s = Self::default();
        *s.lines.lock().unwrap() = lines.into();
        s
    }
}

struct FakePort(FakeStreamer);

impl LinePort for FakePort {
    fn read_line(&mut self) -> io::Result<Option<String>> {
        let line = self.0.lines.lock().unwrap().pop_front();
        if line.is_none() {
            std::thread::sleep(Duration::from_millis(10)); // a read timeout
        }
        Ok(line)
    }

    fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.0.written.lock().unwrap().push(line.to_string());
        Ok(())
    }
}

impl Opener for FakeStreamer {
    fn open(&self, path: &str) -> io::Result<Box<dyn LinePort>> {
        self.opened.lock().unwrap().push(path.to_string());
        if self.broken.lock().unwrap().iter().any(|p| p == path) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{path}: no such device"),
            ));
        }
        Ok(Box::new(FakePort(self.clone())))
    }
}

/// Polls until `f` gives a value or 10 s have passed
pub fn eventually<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    for _ in 0..1000 {
        if let Some(v) = f() {
            return v;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}
