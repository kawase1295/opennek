//! The host's log: the console lines the engine emits, the latest of which it keeps for the UI

use std::collections::VecDeque;

use serde::Serialize;
use serde_json::json;

use crate::radio::RadioStatus;

/// How many lines the engine keeps
pub const CONSOLE_LINES: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConsoleLine {
    pub line: String,
    /// Wall-clock ms
    pub time: i64,
}

/// The most recent lines, oldest first; a full buffer drops the oldest
pub struct ConsoleBuffer {
    lines: VecDeque<ConsoleLine>,
    capacity: usize,
}

impl ConsoleBuffer {
    pub fn new(capacity: usize) -> ConsoleBuffer {
        ConsoleBuffer {
            lines: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, line: ConsoleLine) {
        if self.lines.len() == self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    pub fn lines(&self) -> Vec<ConsoleLine> {
        self.lines.iter().cloned().collect()
    }
}

/// The receiver's state as one line; logged when it changes, not for every frame counted
pub fn radio_line(s: &RadioStatus) -> String {
    format!(
        "sigfox: {}{}{}{}{}",
        json!(s.state).as_str().unwrap_or_default(),
        s.port.as_ref().map(|p| format!(" {p}")).unwrap_or_default(),
        s.fw.as_ref().map(|f| format!(" fw={f}")).unwrap_or_default(),
        s.freq
            .map(|f| format!(" {:.3} MHz", f64::from(f) / 1e6))
            .unwrap_or_default(),
        s.error.as_ref().map(|e| format!(" ({e})")).unwrap_or_default(),
    )
}
