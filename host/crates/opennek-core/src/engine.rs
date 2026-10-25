//! Turns what the radio receives into stored messages and live events

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use sigfox::auth::{PUBLIC_KEY, verify_uplink};
use sigfox::downlink::{downlink_frequency, downlink_window, encode_downlink};
use sigfox::frame::{ControlKind, DeviceId, decode_control};
use tokio::sync::broadcast;

use crate::callbacks::{CallbackResult, applies_to, run_callback};
use crate::console::{CONSOLE_LINES, ConsoleBuffer, ConsoleLine, radio_line};
use crate::downlink::{DownlinkContext, render_downlink};
use crate::payload::decode_payload;
use crate::radio::{DownlinkTx, RadioEvent, RadioMessage, RadioStatus, Transmitter, now_ms};
use crate::store::{
    Auth, Callback, CallbackLog, Device, DownlinkStatus, Message, MessageQuery, NewCallbackLog, NewMessage,
    ReceiverPort, Store,
};

/// The receiver merges repetitions until 10 s after the last one; a repetition later than that is ignored for this long
const RECENT_MS: i64 = 60_000;
/// The station variable (in place of a Sigfox base station ID)
const STATION: &str = "0000";
const EVENT_BUFFER: usize = 256;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum Event {
    Message { message: Message },
    Radio { status: RadioStatus },
    Console { line: String, time: i64 },
    CallbackLog { log: CallbackLog },
}

impl Event {
    /// The SSE event name, which the UI listens to
    pub fn name(&self) -> &'static str {
        match self {
            Event::Message { .. } => "message",
            Event::Radio { .. } => "radio",
            Event::Console { .. } => "console",
            Event::CallbackLog { .. } => "callbackLog",
        }
    }
}

pub struct Engine {
    store: Store,
    events: broadcast::Sender<Event>,
    /// Messages handled lately, by device and counter, with when (wall-clock ms)
    recent: Mutex<HashMap<(DeviceId, u16), i64>>,
    /// Weak: the radio that transmits also feeds the engine
    transmitter: OnceLock<Weak<dyn Transmitter>>,
    /// The latest console lines, for the UI's log
    logs: Mutex<ConsoleBuffer>,
    /// The receiver's state as last logged
    radio_line: Mutex<String>,
}

/// What a downlink needs to know about the uplink it answers
struct Uplink {
    /// None when receiving only: no transmitter, or transmitting off when the uplink came in
    transmitter: Option<Arc<dyn Transmitter>>,
    device: Option<Device>,
    auth: Auth,
    trusted: bool,
    device_id: DeviceId,
    first_frame_end_at: Option<i64>,
    uplink_freq_hz: Option<u32>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Exactly N bytes of hex
fn parse_hex<const N: usize>(hex: &str) -> Option<[u8; N]> {
    let bytes: Option<Vec<u8>> = (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect();
    bytes.filter(|_| hex.len().is_multiple_of(2))?.try_into().ok()
}

impl Engine {
    pub fn new(store: Store) -> Engine {
        Engine {
            store,
            events: broadcast::channel(EVENT_BUFFER).0,
            recent: Mutex::new(HashMap::new()),
            transmitter: OnceLock::new(),
            logs: Mutex::new(ConsoleBuffer::new(CONSOLE_LINES)),
            radio_line: Mutex::new(String::new()),
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    fn emit(&self, e: Event) {
        // no subscriber is fine: nobody is watching
        let _ = self.events.send(e);
    }

    pub fn console(&self, line: String) {
        let time = now_ms();
        let line_for_log = ConsoleLine {
            line: line.clone(),
            time,
        };
        self.logs.lock().unwrap_or_else(|e| e.into_inner()).push(line_for_log);
        self.emit(Event::Console { line, time });
    }

    /// The latest console lines, oldest first
    pub fn logs(&self) -> Vec<ConsoleLine> {
        self.logs.lock().unwrap_or_else(|e| e.into_inner()).lines()
    }

    // Every status goes to the UI; the log gets a line only when the state changes, not for every frame counted
    fn radio_status(&self, status: RadioStatus) {
        let line = radio_line(&status);
        self.emit(Event::Radio { status });
        let changed = {
            let mut last = self.radio_line.lock().unwrap_or_else(|e| e.into_inner());
            let changed = *last != line;
            if changed {
                last.clone_from(&line);
            }
            changed
        };
        if changed {
            self.console(line);
        }
    }

    pub fn handle_radio(self: &Arc<Self>, e: RadioEvent) {
        match e {
            RadioEvent::Status(status) => self.radio_status(status),
            RadioEvent::Log(line) => self.console(line),
            RadioEvent::Found(port) => self.take_kit_port(port),
            RadioEvent::Message(m) => {
                if let Err(e) = self.handle_sigfox(&m) {
                    self.console(format!("error: {e}"));
                }
            }
        }
    }

    // Unless a port was set meanwhile
    fn take_kit_port(&self, port: String) {
        let saved = self.store.settings().and_then(|mut s| {
            if s.sigfox_port != ReceiverPort::Unset {
                return Ok(false);
            }
            s.sigfox_port = ReceiverPort::Port(port.clone());
            self.store.save_settings(&s).map(|()| true)
        });
        match saved {
            Ok(true) => self.console(format!("sigfox: found the XIAO kit on {port}")),
            Ok(false) => {}
            Err(e) => self.console(format!("error: {e}")),
        }
    }

    // Repetitions are already merged by the receiver
    fn handle_sigfox(self: &Arc<Self>, r: &RadioMessage) -> crate::store::Result<()> {
        let m = &r.message;
        let now = now_ms();
        {
            let mut recent = self.recent.lock().unwrap_or_else(|e| e.into_inner());
            recent.retain(|_, &mut t| now - t <= RECENT_MS);
            if recent.insert((m.device_id, m.counter), now).is_some() {
                self.console(format!(
                    "sigfox: late repetition of {} seq={} ignored",
                    m.device_id, m.counter
                ));
                return Ok(());
            }
        }
        let id = m.device_id.to_string();
        self.store.ensure_device(&id, now)?;
        let device = self.store.get_device(&id)?;
        let auth = Self::auth(device.as_ref(), m);
        self.store.touch_device(&id, m.counter, r.received_at)?;
        let control = m.control.then(|| decode_control(&m.payload)).flatten();
        let confirmation = control
            .as_ref()
            .is_some_and(|c| c.kind == ControlKind::DownlinkConfirmation);
        let decoded = control.map(|c| {
            let mut v = json!({
                "type": match c.kind {
                    ControlKind::KeepAlive => "keepAlive",
                    ControlKind::DownlinkConfirmation => "downlinkConfirmation",
                },
                "voltageIdleMv": c.voltage_idle_mv,
                "voltageTxMv": c.voltage_tx_mv,
                "temperatureC": c.temperature_c,
            });
            if let (Some(rssi), Value::Object(o)) = (c.downlink_rssi_dbm, &mut v) {
                o.insert("downlinkRssiDbm".into(), rssi.into());
            }
            v
        });
        let payload_config = device.as_ref().map_or("", |d| d.payload_config.as_str());
        // an OOB frame that is not a known control message falls through to the device's format
        let decoded = decoded.or_else(|| {
            (!payload_config.is_empty() && !m.payload.is_empty()).then(|| {
                match decode_payload(payload_config, &m.payload) {
                    Ok(fields) => Value::Object(fields),
                    Err(e) => json!({ "_error": e }),
                }
            })
        });
        let message = self.store.insert_message(&NewMessage {
            device_id: id,
            time: r.received_at,
            seq: m.counter,
            // a single-bit message (AT$SB) has no data; like Sigfox, its value is not put in data
            data: hex(&m.payload),
            ack: m.downlink,
            // keep-alive and downlink confirmation
            oob: m.control,
            // halves up, as Math.round in the TypeScript host: the streamer gives 0.5 dB steps
            rssi: (m.rssi + 0.5).floor() as i32,
            nb_frames: m.ranks.len() as u32,
            auth,
            station: STATION.into(),
            decoded,
            downlink: None,
            downlink_status: if m.downlink {
                DownlinkStatus::Pending
            } else {
                DownlinkStatus::None
            },
        })?;
        self.emit(Event::Message {
            message: message.clone(),
        });

        // A frame whose key does not match may be spoofed: no callbacks for it
        let settings = self.store.settings()?;
        let trusted = auth == Auth::Ok || (auth != Auth::Bad && settings.accept_unknown_devices);
        let callbacks: Vec<Callback> = if trusted {
            self.store
                .list_callbacks()?
                .into_iter()
                .filter(|c| c.input.enabled && applies_to(c, &message.fields.device_id))
                .collect()
        } else {
            vec![]
        };
        // A device confirms a downlink it received with a control message at the next counter
        if confirmation && auth != Auth::Bad {
            self.confirm_downlink(&message.fields.device_id, m.counter)?;
        }
        // Decided once: a transmitter picked up later could run on the receiver's thread
        let transmitter = self
            .transmitter()
            .filter(|_| message.fields.ack && settings.sigfox_downlink);
        let waits = !callbacks.is_empty() || transmitter.is_some();
        let uplink = Uplink {
            transmitter,
            device,
            auth,
            trusted,
            device_id: m.device_id,
            first_frame_end_at: r.first_frame_end_at,
            uplink_freq_hz: r.uplink_freq_hz,
        };
        let engine = self.clone();
        let finish = move || {
            let name = uplink.device.as_ref().map(|d| d.name.clone()).unwrap_or_default();
            let results = engine.run_callbacks(&callbacks, &message, &name, settings.downlink_timeout_ms);
            if let Err(e) = engine.settle_downlink(&message, &uplink, &results) {
                engine.console(format!("error: {e}"));
            }
        };
        // Callbacks wait for the user's server and a downlink for the device's receive window, neither of which may
        // hold up the receiver
        if waits {
            std::thread::spawn(finish);
        } else {
            finish();
        }
        Ok(())
    }

    /// All callbacks at once; each answer is logged as it comes
    fn run_callbacks(
        &self,
        callbacks: &[Callback],
        message: &Message,
        device_name: &str,
        timeout_ms: u32,
    ) -> Vec<CallbackResult> {
        let (done, answers) = std::sync::mpsc::channel();
        std::thread::scope(|s| {
            for cb in callbacks {
                let done = done.clone();
                s.spawn(move || done.send((cb.id, run_callback(cb, message, device_name, timeout_ms))));
            }
            drop(done);
            answers
                .iter()
                .map(|(callback_id, r)| self.log_callback(callback_id, message.id, r))
                .collect()
        })
    }

    fn log_callback(&self, callback_id: i64, message_id: i64, r: CallbackResult) -> CallbackResult {
        let log = self.store.insert_log(&NewCallbackLog {
            callback_id,
            message_id,
            time: now_ms(),
            status: r.status.map(i64::from),
            ok: r.ok,
            error: r.error.clone(),
            duration_ms: r.duration_ms,
            response: r.response.clone(),
        });
        match log {
            Ok(log) => self.emit(Event::CallbackLog { log }),
            Err(e) => self.console(format!("error: {e}")),
        }
        r
    }

    fn update_downlink(&self, id: i64, downlink: Option<String>, status: DownlinkStatus) -> crate::store::Result<()> {
        if let Some(message) = self.store.update_downlink(id, downlink, status)? {
            self.emit(Event::Message { message });
        }
        Ok(())
    }

    fn confirm_downlink(&self, device_id: &str, counter: u16) -> crate::store::Result<()> {
        let seq = (counter + 0xfff) & 0xfff;
        let query = MessageQuery {
            device_id: Some(device_id.into()),
            limit: Some(20),
            ..Default::default()
        };
        let sent = self
            .store
            .list_messages(&query)?
            .into_iter()
            .find(|m| m.fields.seq == seq && m.fields.downlink_status == DownlinkStatus::Sent);
        match sent {
            Some(m) => self.update_downlink(m.id, m.fields.downlink, DownlinkStatus::Acked),
            None => Ok(()),
        }
    }

    /// The downlink data (the device's static template or its callbacks) and the key to sign it with
    fn settle_downlink(&self, message: &Message, up: &Uplink, results: &[CallbackResult]) -> crate::store::Result<()> {
        if !message.fields.ack {
            return Ok(());
        }
        let f = &message.fields;
        let data = up.device.as_ref().and_then(|d| match d.downlink_mode.as_str() {
            "static" if !d.downlink_data.is_empty() => {
                let ctx = DownlinkContext {
                    time: f.time,
                    station: &f.station,
                    rssi: f.rssi,
                };
                render_downlink(&d.downlink_data, &ctx).ok()
            }
            "callback" => results.iter().find_map(|r| r.downlink.clone()),
            _ => None,
        });
        // A device in public key mode gets the public key; while unknown devices are refused it gets nothing, as
        // with callbacks
        let key = match up.auth {
            Auth::Ok => up.device.as_ref().and_then(|d| parse_hex(&d.key)),
            Auth::Public if up.trusted => Some(PUBLIC_KEY),
            _ => None,
        };
        self.sigfox_downlink(message, up, data, key)
    }

    // Waits for the device's receive window (T_W after the end of frame 1) and hands the frame to the transmitter
    fn sigfox_downlink(
        &self,
        message: &Message,
        up: &Uplink,
        data: Option<String>,
        key: Option<[u8; 16]>,
    ) -> crate::store::Result<()> {
        let f = &message.fields;
        let name = format!("sigfox downlink for {} seq={}", f.device_id, f.seq);
        // No transmitter or transmitting off: receive only (the callbacks were given ack=true all the same)
        let Some(tx) = up.transmitter.as_ref() else {
            return self.update_downlink(message.id, None, DownlinkStatus::Unsupported);
        };
        let payload: Option<[u8; 8]> = data.as_deref().and_then(parse_hex);
        let (Some(data), Some(payload), Some(key)) = (data.clone(), payload, key) else {
            if data.is_some() {
                self.console(format!(
                    "{name} skipped: uplink not authenticated (auth={})",
                    json!(up.auth).as_str().unwrap_or_default()
                ));
            }
            return self.update_downlink(message.id, None, DownlinkStatus::Nodata);
        };
        let (Some(first_frame_end_at), Some(uplink_freq_hz)) = (up.first_frame_end_at, up.uplink_freq_hz) else {
            let why = if up.first_frame_end_at.is_none() {
                "frame 1 not received"
            } else {
                "frequency of frame 1 unknown"
            };
            self.console(format!("{name} skipped: {why}"));
            return self.update_downlink(message.id, None, DownlinkStatus::Unsupported);
        };
        let window = downlink_window(first_frame_end_at);
        if now_ms() > window.deadline {
            self.console(format!("{name} skipped: the device's receive window has closed"));
            return self.update_downlink(message.id, Some(data), DownlinkStatus::Failed);
        }
        let freq_hz = downlink_frequency(uplink_freq_hz);
        let frame = encode_downlink(up.device_id, f.seq, &payload, &key);
        std::thread::sleep(Duration::from_millis(u64::try_from(window.at - now_ms()).unwrap_or(0)));
        // It puts a signal on air, so every transmission is logged
        self.console(format!(
            "{name}: TX {:.6} MHz data={data} frame={}",
            f64::from(freq_hz) / 1e6,
            hex(&frame)
        ));
        let sent = tx.transmit(&DownlinkTx {
            device_id: up.device_id,
            counter: f.seq,
            freq_hz,
            frame,
            deadline: window.deadline,
        });
        self.console(format!("{name}: {}", if sent { "sent" } else { "not sent" }));
        self.update_downlink(
            message.id,
            Some(data),
            if sent {
                DownlinkStatus::Sent
            } else {
                DownlinkStatus::Failed
            },
        )
    }

    /// The transmitter for downlinks, once the host has wired one
    pub fn set_transmitter(&self, tx: Arc<dyn Transmitter>) {
        let _ = self.transmitter.set(Arc::downgrade(&tx));
    }

    fn transmitter(&self) -> Option<Arc<dyn Transmitter>> {
        self.transmitter.get().and_then(Weak::upgrade)
    }

    // A registered key decides. Anyone can sign with the public key, so it counts as 'public', like an unknown device
    fn auth(device: Option<&Device>, m: &sigfox::receiver::SigfoxMessage) -> Auth {
        match device.and_then(|d| parse_hex(&d.key)) {
            Some(key) if verify_uplink(&key, &m.fields(), &m.auth) => Auth::Ok,
            Some(_) => Auth::Bad,
            None if verify_uplink(&PUBLIC_KEY, &m.fields(), &m.auth) => Auth::Public,
            None => Auth::Unverified,
        }
    }
}
