//! Turns what the radio receives into stored messages and live events

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{Value, json};
use sigfox::auth::{PUBLIC_KEY, verify_uplink};
use sigfox::frame::{ControlKind, DeviceId, decode_control};
use tokio::sync::broadcast;

use crate::payload::decode_payload;
use crate::radio::{RadioEvent, RadioMessage, RadioStatus, now_ms};
use crate::store::{Auth, Device, DownlinkStatus, Message, NewMessage, Store};

/// The receiver merges repetitions until 10 s after the last one; a repetition later than that is ignored for this long
const RECENT_MS: i64 = 60_000;
/// The station variable (in place of a Sigfox base station ID)
const STATION: &str = "0000";
const EVENT_BUFFER: usize = 256;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    Message { message: Message },
    Radio { status: RadioStatus },
    Console { line: String, time: i64 },
}

impl Event {
    /// The SSE event name, which the UI listens to
    pub fn name(&self) -> &'static str {
        match self {
            Event::Message { .. } => "message",
            Event::Radio { .. } => "radio",
            Event::Console { .. } => "console",
        }
    }
}

pub struct Engine {
    store: Store,
    events: broadcast::Sender<Event>,
    /// Messages handled lately, by device and counter, with when (wall-clock ms)
    recent: Mutex<HashMap<(DeviceId, u16), i64>>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_key(key: &str) -> Option<[u8; 16]> {
    let bytes: Option<Vec<u8>> = (0..key.len() / 2)
        .map(|i| u8::from_str_radix(key.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect();
    bytes?.try_into().ok()
}

impl Engine {
    pub fn new(store: Store) -> Engine {
        Engine {
            store,
            events: broadcast::channel(EVENT_BUFFER).0,
            recent: Mutex::new(HashMap::new()),
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
        self.emit(Event::Console { line, time: now_ms() });
    }

    pub fn handle_radio(&self, e: RadioEvent) {
        match e {
            RadioEvent::Status(status) => self.emit(Event::Radio { status }),
            RadioEvent::Log(line) => self.console(line),
            RadioEvent::Message(m) => {
                if let Err(e) = self.handle_sigfox(&m) {
                    self.console(format!("error: {e}"));
                }
            }
        }
    }

    // Repetitions are already merged by the receiver
    fn handle_sigfox(&self, r: &RadioMessage) -> crate::store::Result<()> {
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
        let decoded = m.control.then(|| decode_control(&m.payload)).flatten().map(|c| {
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
        let id = message.id;
        self.emit(Event::Message { message });
        // Downlinks are sent from #33 on; until then the host only receives, as with no transmitter
        if m.downlink
            && let Some(message) = self.store.set_downlink_status(id, DownlinkStatus::Unsupported)?
        {
            self.emit(Event::Message { message });
        }
        Ok(())
    }

    // A registered key decides. Anyone can sign with the public key, so it counts as 'public', like an unknown device
    fn auth(device: Option<&Device>, m: &sigfox::receiver::SigfoxMessage) -> Auth {
        match device.and_then(|d| parse_key(&d.key)) {
            Some(key) if verify_uplink(&key, &m.fields(), &m.auth) => Auth::Ok,
            Some(_) => Auth::Bad,
            None if verify_uplink(&PUBLIC_KEY, &m.fields(), &m.auth) => Auth::Public,
            None => Auth::Unverified,
        }
    }
}
