//! The host log in the data dir: every console line with its date and time, so there is something to read after the
//! process has ended (the desktop app has no terminal)

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tokio::sync::{broadcast, oneshot};
use tokio::task::JoinHandle;

use crate::engine::Event;
use crate::radio::now_ms;

/// A file this big is renamed to `opennek.log.1` (replacing the previous one) and a new one is started
pub const LIMIT: u64 = 5 * 1024 * 1024;

pub fn log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("logs").join("opennek.log")
}

pub struct LogFile {
    path: PathBuf,
    limit: u64,
    /// Set by a failed write, so a failure is reported once and not for every line
    failing: bool,
}

impl LogFile {
    pub fn new(path: PathBuf, limit: u64) -> LogFile {
        LogFile {
            path,
            limit,
            failing: false,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one line stamped with `time` (wall-clock ms); the error is returned only for the first failure in a
    /// row, for the caller to report
    pub fn append(&mut self, time: i64, line: &str) -> Option<io::Error> {
        match self.try_append(time, line) {
            Ok(()) => {
                self.failing = false;
                None
            }
            Err(e) => (!std::mem::replace(&mut self.failing, true)).then_some(e),
        }
    }

    /// Like `append`, reporting a failure on stderr; writing the log never stops the host
    pub fn write(&mut self, time: i64, line: &str) {
        if let Some(e) = self.append(time, line) {
            eprintln!("opennek: cannot write the log {}: {e}", self.path.display());
        }
    }

    // Opened for each line, so a file moved or removed meanwhile is started again
    fn try_append(&self, time: i64, line: &str) -> io::Result<()> {
        let stamp = chrono::DateTime::from_timestamp_millis(time)
            .unwrap_or_default()
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S%.3f");
        let text = format!("{stamp} {line}\n");
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let size = fs::metadata(&self.path).map_or(0, |m| m.len());
        if size > 0 && size + text.len() as u64 > self.limit {
            let mut old = self.path.clone().into_os_string();
            old.push(".1");
            fs::rename(&self.path, old)?;
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?
            .write_all(text.as_bytes())
    }
}

/// Writes the console lines to a log file until stopped
pub struct Logger {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl Logger {
    pub fn spawn(mut log: LogFile, mut events: broadcast::Receiver<Event>, stopped: String) -> Logger {
        let (stop, mut stopping) = oneshot::channel();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    // the lines already sent are written before stopping
                    biased;
                    e = events.recv() => match e {
                        Ok(Event::Console { line, time }) => log.write(time, &line),
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(n)) => log.write(now_ms(), &format!("({n} events missed)")),
                        Err(broadcast::error::RecvError::Closed) => break,
                    },
                    _ = &mut stopping => break,
                }
            }
            log.write(now_ms(), &stopped);
        });
        Logger { stop, task }
    }

    /// Writes the lines sent so far, then the stop line
    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}
