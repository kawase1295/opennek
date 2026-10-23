//! The SQLite database, shared with the TypeScript host: the same schema, so either opens the other's file

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Result<T> = rusqlite::Result<T>;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS devices (
  id TEXT PRIMARY KEY, name TEXT NOT NULL DEFAULT '', key TEXT NOT NULL DEFAULT '',
  downlink_mode TEXT NOT NULL DEFAULT 'none', downlink_data TEXT NOT NULL DEFAULT '',
  payload_config TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL, last_seen INTEGER, last_seq INTEGER
);
CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT, device_id TEXT NOT NULL, time INTEGER NOT NULL, seq INTEGER NOT NULL,
  data TEXT NOT NULL, ack INTEGER NOT NULL, oob INTEGER NOT NULL, rssi INTEGER NOT NULL, nb_frames INTEGER NOT NULL,
  auth TEXT NOT NULL, station TEXT NOT NULL, decoded TEXT,
  downlink TEXT, downlink_status TEXT NOT NULL DEFAULT 'none'
);
CREATE INDEX IF NOT EXISTS messages_device ON messages (device_id, id);
CREATE TABLE IF NOT EXISTS callbacks (
  id INTEGER PRIMARY KEY AUTOINCREMENT, data TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS callback_logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT, callback_id INTEGER NOT NULL, message_id INTEGER NOT NULL, time INTEGER NOT NULL,
  status INTEGER, ok INTEGER NOT NULL, error TEXT, duration_ms INTEGER NOT NULL, response TEXT
);
CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK (id = 1), data TEXT NOT NULL);
";

const MAX_MESSAGES: i64 = 10_000;
const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 1000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub downlink_timeout_ms: u32,
    /// Show and forward messages of devices that are not registered
    pub accept_unknown_devices: bool,
    /// Port of the receiver; empty when it is not used
    pub sigfox_port: String,
    /// RC3 uplink centre frequency, Hz
    pub sigfox_freq: u32,
    /// Transmit downlinks to real Sigfox devices. It puts a signal on air, so it is off until enabled
    pub sigfox_downlink: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            downlink_timeout_ms: 5000,
            accept_unknown_devices: true,
            sigfox_port: String::new(),
            sigfox_freq: 923_200_000,
            sigfox_downlink: false,
        }
    }
}

impl Settings {
    /// These settings with the fields of `patch` (a JSON object) replaced, checked as the TypeScript host does
    pub fn merged(&self, patch: Value) -> std::result::Result<Settings, String> {
        let Value::Object(patch) = patch else {
            return Err("body: expected an object".into());
        };
        let Ok(Value::Object(mut merged)) = serde_json::to_value(self) else {
            unreachable!("settings are an object")
        };
        merged.extend(patch);
        let s: Settings = serde_json::from_value(Value::Object(merged)).map_err(|e| format!("body: {e}"))?;
        s.check()?;
        Ok(s)
    }

    fn check(&self) -> std::result::Result<(), String> {
        if !(500..=30_000).contains(&self.downlink_timeout_ms) {
            return Err("downlinkTimeoutMs: must be between 500 and 30000".into());
        }
        if !(862_000_000..=928_000_000).contains(&self.sigfox_freq) {
            return Err("sigfoxFreq: must be between 862000000 and 928000000".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    /// 32 hex digits, or empty when not known
    pub key: String,
    pub downlink_mode: String,
    pub downlink_data: String,
    pub payload_config: String,
    pub created_at: i64,
    pub last_seen: Option<i64>,
    pub last_seq: Option<i64>,
}

/// How an uplink's authentication field checked out
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Auth {
    /// The registered key verifies it
    Ok,
    /// The registered key does not
    Bad,
    /// No key registered, but the public key verifies it
    Public,
    /// No key registered
    Unverified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownlinkStatus {
    None,
    Pending,
    Sent,
    /// The device confirmed it with a control message
    Acked,
    Failed,
    Nodata,
    Unsupported,
}

impl DownlinkStatus {
    fn as_str(self) -> &'static str {
        match self {
            DownlinkStatus::None => "none",
            DownlinkStatus::Pending => "pending",
            DownlinkStatus::Sent => "sent",
            DownlinkStatus::Acked => "acked",
            DownlinkStatus::Failed => "failed",
            DownlinkStatus::Nodata => "nodata",
            DownlinkStatus::Unsupported => "unsupported",
        }
    }
}

impl Auth {
    fn as_str(self) -> &'static str {
        match self {
            Auth::Ok => "ok",
            Auth::Bad => "bad",
            Auth::Public => "public",
            Auth::Unverified => "unverified",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewMessage {
    pub device_id: String,
    /// Wall-clock ms
    pub time: i64,
    pub seq: u16,
    /// Payload as hex
    pub data: String,
    /// The device asked for a downlink
    pub ack: bool,
    /// A control message (keep-alive, downlink confirmation)
    pub oob: bool,
    pub rssi: i32,
    pub nb_frames: u32,
    pub auth: Auth,
    pub station: String,
    pub decoded: Option<Value>,
    pub downlink: Option<String>,
    pub downlink_status: DownlinkStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: i64,
    #[serde(flatten)]
    pub fields: NewMessage,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MessageQuery {
    pub device_id: Option<String>,
    /// Only messages with a smaller id (paging)
    pub before: Option<i64>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallbackLog {
    pub id: i64,
    pub callback_id: i64,
    pub message_id: i64,
    pub time: i64,
    pub status: Option<i64>,
    pub ok: bool,
    pub error: Option<String>,
    pub duration_ms: i64,
    pub response: Option<String>,
}

fn parse_text<T: for<'de> Deserialize<'de>>(row: &Row, col: &str) -> Result<T> {
    let s: String = row.get(col)?;
    serde_json::from_value(Value::String(s.clone()))
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
}

fn to_device(r: &Row) -> Result<Device> {
    Ok(Device {
        id: r.get("id")?,
        name: r.get("name")?,
        key: r.get("key")?,
        downlink_mode: r.get("downlink_mode")?,
        downlink_data: r.get("downlink_data")?,
        payload_config: r.get("payload_config")?,
        created_at: r.get("created_at")?,
        last_seen: r.get("last_seen")?,
        last_seq: r.get("last_seq")?,
    })
}

fn to_message(r: &Row) -> Result<Message> {
    let decoded: Option<String> = r.get("decoded")?;
    Ok(Message {
        id: r.get("id")?,
        fields: NewMessage {
            device_id: r.get("device_id")?,
            time: r.get("time")?,
            seq: r.get("seq")?,
            data: r.get("data")?,
            ack: r.get("ack")?,
            oob: r.get("oob")?,
            rssi: r.get("rssi")?,
            nb_frames: r.get("nb_frames")?,
            auth: parse_text(r, "auth")?,
            station: r.get("station")?,
            decoded: decoded.and_then(|d| serde_json::from_str(&d).ok()),
            downlink: r.get("downlink")?,
            downlink_status: parse_text(r, "downlink_status")?,
        },
    })
}

fn to_log(r: &Row) -> Result<CallbackLog> {
    Ok(CallbackLog {
        id: r.get("id")?,
        callback_id: r.get("callback_id")?,
        message_id: r.get("message_id")?,
        time: r.get("time")?,
        status: r.get("status")?,
        ok: r.get("ok")?,
        error: r.get("error")?,
        duration_ms: r.get("duration_ms")?,
        response: r.get("response")?,
    })
}

pub struct Store {
    db: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut db: Connection) -> Result<Store> {
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.execute_batch(SCHEMA)?;
        let cols: Vec<String> = db
            .prepare("PRAGMA table_info(messages)")?
            .query_map([], |r| r.get("name"))?
            .collect::<Result<_>>()?;
        // DBs from before the ESP-NOW network was removed (#19) mark each message with the radio it came in on.
        // Messages of the ESP-NOW devices cannot be told apart from Sigfox ones without that column, so drop them first
        if cols.iter().any(|c| c == "radio") {
            let tx = db.transaction()?;
            tx.execute_batch(
                "DELETE FROM callback_logs WHERE message_id IN (SELECT id FROM messages WHERE radio != 'sigfox');
                 DELETE FROM messages WHERE radio != 'sigfox';
                 ALTER TABLE messages DROP COLUMN radio;",
            )?;
            tx.commit()?;
        }
        // src was the MAC address of the ESP-NOW device, where its downlinks went
        if cols.iter().any(|c| c == "src") {
            db.execute_batch("ALTER TABLE messages DROP COLUMN src")?;
        }
        Ok(Store { db: Mutex::new(db) })
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        // a panic while holding the lock leaves SQLite consistent; keep serving
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    // --- settings

    /// The saved settings over the defaults; fields that do not parse fall back to the defaults as a whole
    pub fn settings(&self) -> Result<Settings> {
        let data: Option<String> = self
            .db()
            .query_row("SELECT data FROM settings WHERE id = 1", [], |r| r.get(0))
            .optional()?;
        let saved = data
            .and_then(|d| serde_json::from_str::<Value>(&d).ok())
            .unwrap_or(Value::Object(Default::default()));
        Ok(Settings::default().merged(saved).unwrap_or_default())
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        let data = serde_json::to_string(s).expect("settings serialize");
        self.db()
            .execute("INSERT OR REPLACE INTO settings (id, data) VALUES (1, ?)", [data])?;
        Ok(())
    }

    // --- devices

    pub fn list_devices(&self) -> Result<Vec<Device>> {
        self.db()
            .prepare("SELECT * FROM devices ORDER BY id")?
            .query_map([], to_device)?
            .collect()
    }

    pub fn get_device(&self, id: &str) -> Result<Option<Device>> {
        self.db()
            .query_row("SELECT * FROM devices WHERE id = ?", [id], to_device)
            .optional()
    }

    /// Registers a device seen on air, with nothing configured, unless it is known
    pub fn ensure_device(&self, id: &str, now: i64) -> Result<()> {
        self.db().execute(
            "INSERT INTO devices (id, created_at) VALUES (?, ?) ON CONFLICT (id) DO NOTHING",
            params![id, now],
        )?;
        Ok(())
    }

    pub fn touch_device(&self, id: &str, seq: u16, time: i64) -> Result<()> {
        self.db().execute(
            "UPDATE devices SET last_seen = ?, last_seq = ? WHERE id = ?",
            params![time, seq, id],
        )?;
        Ok(())
    }

    // --- messages

    pub fn insert_message(&self, m: &NewMessage) -> Result<Message> {
        let db = self.db();
        let decoded = m.decoded.as_ref().map(Value::to_string);
        let r = db.query_row(
            "INSERT INTO messages (device_id, time, seq, data, ack, oob, rssi, nb_frames, auth, station, decoded, downlink,
               downlink_status)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING *",
            params![
                m.device_id,
                m.time,
                m.seq,
                m.data,
                m.ack,
                m.oob,
                m.rssi,
                m.nb_frames,
                m.auth.as_str(),
                m.station,
                decoded,
                m.downlink,
                m.downlink_status.as_str()
            ],
            to_message,
        )?;
        db.execute("DELETE FROM messages WHERE id <= ?", [r.id - MAX_MESSAGES])?;
        Ok(r)
    }

    pub fn set_downlink_status(&self, id: i64, status: DownlinkStatus) -> Result<Option<Message>> {
        self.db().execute(
            "UPDATE messages SET downlink_status = ? WHERE id = ?",
            params![status.as_str(), id],
        )?;
        self.get_message(id)
    }

    pub fn get_message(&self, id: i64) -> Result<Option<Message>> {
        self.db()
            .query_row("SELECT * FROM messages WHERE id = ?", [id], to_message)
            .optional()
    }

    pub fn list_messages(&self, q: &MessageQuery) -> Result<Vec<Message>> {
        let limit = q.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
        self.db()
            .prepare(
                "SELECT * FROM messages WHERE (?1 IS NULL OR device_id = ?1) AND (?2 IS NULL OR id < ?2)
                 ORDER BY id DESC LIMIT ?3",
            )?
            .query_map(params![q.device_id, q.before, limit], to_message)?
            .collect()
    }

    // --- callbacks (edited from #33 on; listed as stored)

    pub fn list_callbacks(&self) -> Result<Vec<Value>> {
        let rows: Vec<(i64, String)> = self
            .db()
            .prepare("SELECT id, data FROM callbacks ORDER BY id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_>>()?;
        Ok(rows
            .into_iter()
            .map(|(id, data)| {
                let mut v = serde_json::from_str(&data).unwrap_or_else(|_| Value::Object(Default::default()));
                if let Value::Object(o) = &mut v {
                    o.insert("id".into(), id.into());
                }
                v
            })
            .collect())
    }

    pub fn list_logs(
        &self,
        callback_id: Option<i64>,
        message_id: Option<i64>,
        limit: Option<u32>,
    ) -> Result<Vec<CallbackLog>> {
        let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
        self.db()
            .prepare(
                "SELECT * FROM callback_logs WHERE (?1 IS NULL OR callback_id = ?1) AND (?2 IS NULL OR message_id = ?2)
                 ORDER BY id DESC LIMIT ?3",
            )?
            .query_map(params![callback_id, message_id, limit], to_log)?
            .collect()
    }
}
