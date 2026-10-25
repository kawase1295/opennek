//! The web UI and the JSON API it uses, with the same paths and shapes as the TypeScript host

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{self, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Serialize;
use serde_json::{Value, json};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};

use crate::callbacks::{CallbackResult, applies_to, dummy_message, run_callback};
use crate::engine::Event;
use crate::host::Host;
use crate::radio::{RadioStatus, now_ms};
use crate::serial::list_serial_ports;
use crate::store::{Callback, CallbackInput, CallbackLog, DeviceInput, Message, MessageQuery, Settings};

// The build of host/ui (`npm run build`), embedded so the binary needs no file next to it; build.rs checks it is there
const INDEX_HTML: &str = include_str!("../../../ui/dist/index.html");
const APP_JS: &str = include_str!("../../../ui/dist/app.js");
const APP_CSS: &str = include_str!("../../../ui/dist/app.css");

const JS: &str = "text/javascript; charset=utf-8";
/// A comment this often keeps proxies from closing the event stream
const SSE_PING: Duration = Duration::from_secs(25);

type Shared = State<Arc<Host>>;

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

type ApiResult = Result<Json<Value>, ApiError>;

fn to_json(v: impl serde::Serialize) -> ApiResult {
    serde_json::to_value(v)
        .map(Json)
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub fn router(host: Arc<Host>) -> Router {
    let api = Router::new()
        .route("/status", get(status))
        .route("/settings", get(settings).put(save_settings))
        .route("/serial/ports", get(serial_ports))
        .route("/devices", get(devices))
        .route("/devices/{id}", put(save_device).delete(delete_device))
        .route("/messages", get(messages).delete(clear_messages))
        .route("/messages.csv", get(messages_csv))
        .route("/callbacks", get(callbacks).post(create_callback))
        .route("/callbacks/{id}", put(update_callback).delete(delete_callback))
        .route("/callbacks/{id}/test", post(test_callback))
        .route("/callback-logs", get(callback_logs))
        .route("/logs", get(logs))
        .route("/events", get(events));
    Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route("/app.js", get(|| async { ([(header::CONTENT_TYPE, JS)], APP_JS) }))
        .route(
            "/app.css",
            get(|| async { ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], APP_CSS) }),
        )
        .nest("/api", api)
        .with_state(host)
}

/// GET /api/status
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub version: String,
    pub radio: RadioStatus,
    pub settings: Settings,
    /// Where the host writes its log, once it does
    pub log_file: Option<String>,
}

async fn status(State(host): Shared) -> ApiResult {
    to_json(Status {
        version: env!("CARGO_PKG_VERSION").into(),
        radio: host.radio.status(),
        settings: host.engine.store().settings()?,
        log_file: host.log_file.get().map(|p| p.display().to_string()),
    })
}

async fn settings(State(host): Shared) -> ApiResult {
    to_json(host.engine.store().settings()?)
}

async fn save_settings(State(host): Shared, Json(patch): Json<Value>) -> ApiResult {
    let store = host.engine.store();
    let before = store.settings()?;
    let after = before.merged(patch).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    store.save_settings(&after)?;
    if after.sigfox_port != before.sigfox_port || after.sigfox_freq != before.sigfox_freq {
        host.radio.reconnect();
    }
    host.radio.set_follow(after.sigfox_downlink);
    to_json(after)
}

async fn serial_ports() -> ApiResult {
    let ports = tokio::task::spawn_blocking(list_serial_ports)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    to_json(ports)
}

async fn devices(State(host): Shared) -> ApiResult {
    to_json(host.engine.store().list_devices()?)
}

async fn save_device(State(host): Shared, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let input = DeviceInput::parse(&id, body).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    to_json(host.engine.store().upsert_device(&input, now_ms())?)
}

async fn delete_device(State(host): Shared, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    host.engine.store().delete_device(&id.to_uppercase())?;
    Ok(StatusCode::NO_CONTENT)
}

fn device_param(q: &HashMap<String, String>) -> Option<String> {
    q.get("device").filter(|d| !d.is_empty()).map(|d| d.to_uppercase())
}

async fn clear_messages(State(host): Shared, Query(q): Query<HashMap<String, String>>) -> Result<StatusCode, ApiError> {
    host.engine.store().clear_messages(device_param(&q).as_deref())?;
    Ok(StatusCode::NO_CONTENT)
}

const CSV_COLUMNS: [&str; 13] = [
    "id",
    "time",
    "deviceId",
    "seq",
    "data",
    "decoded",
    "ack",
    "oob",
    "rssi",
    "nbFrames",
    "auth",
    "downlink",
    "downlinkStatus",
];

fn csv_cell(v: &Value) -> String {
    let s = match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if s.contains(['"', ',', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

async fn messages_csv(State(host): Shared, Query(q): Query<HashMap<String, String>>) -> Result<Response, ApiError> {
    let query = MessageQuery {
        device_id: device_param(&q),
        before: None,
        limit: Some(1000),
    };
    let mut lines = vec![CSV_COLUMNS.join(",")];
    for m in host.engine.store().list_messages(&query)? {
        let row = serde_json::to_value(&m).unwrap_or_default();
        let time = chrono::DateTime::from_timestamp_millis(m.fields.time).unwrap_or_default();
        let cells = CSV_COLUMNS.map(|k| match k {
            "time" => time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            _ => csv_cell(&row[k]),
        });
        lines.push(cells.join(","));
    }
    let headers = [
        (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
        (
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"opennek-messages.csv\"",
        ),
    ];
    Ok((headers, format!("{}\n", lines.join("\n"))).into_response())
}

fn number<T: std::str::FromStr>(q: &HashMap<String, String>, k: &str) -> Option<T> {
    q.get(k).and_then(|v| v.parse().ok())
}

async fn messages(State(host): Shared, Query(q): Query<HashMap<String, String>>) -> ApiResult {
    let query = MessageQuery {
        device_id: device_param(&q),
        before: number(&q, "before"),
        limit: number(&q, "limit"),
    };
    to_json(host.engine.store().list_messages(&query)?)
}

/// GET /api/callbacks: each callback with its newest answer
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CallbackSummary {
    #[serde(flatten)]
    pub callback: Callback,
    pub last_log: Option<CallbackLog>,
}

async fn callbacks(State(host): Shared) -> ApiResult {
    let store = host.engine.store();
    let mut last = store.last_logs()?;
    let list: Vec<CallbackSummary> = store
        .list_callbacks()?
        .into_iter()
        .map(|callback| CallbackSummary {
            last_log: last.remove(&callback.id),
            callback,
        })
        .collect();
    to_json(list)
}

fn callback_input(body: Value) -> Result<CallbackInput, ApiError> {
    CallbackInput::parse(body).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))
}

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "not found".into())
}

async fn create_callback(State(host): Shared, Json(body): Json<Value>) -> Result<(StatusCode, Json<Value>), ApiError> {
    let created = host.engine.store().create_callback(&callback_input(body)?)?;
    Ok((StatusCode::CREATED, to_json(created)?))
}

async fn update_callback(State(host): Shared, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let id = id.parse().map_err(|_| not_found())?;
    let updated = host.engine.store().update_callback(id, &callback_input(body)?)?;
    to_json(updated.ok_or_else(not_found)?)
}

async fn delete_callback(State(host): Shared, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if let Ok(id) = id.parse() {
        host.engine.store().delete_callback(id)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/callbacks/{id}/test
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TestResult {
    /// The message it was tried with
    pub message: Message,
    pub result: CallbackResult,
}

// Tries a callback with the latest message it applies to (or a made-up one); not logged
async fn test_callback(State(host): Shared, Path(id): Path<String>) -> ApiResult {
    let id = id.parse().map_err(|_| not_found())?;
    let store = host.engine.store();
    let cb = store.get_callback(id)?.ok_or_else(not_found)?;
    if !cb.input.is_sent() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "this callback is not sent by OpenNEK: only DATA callbacks to a URL are".into(),
        ));
    }
    let recent = store.list_messages(&MessageQuery {
        limit: Some(50),
        ..Default::default()
    })?;
    let message = recent
        .into_iter()
        .find(|m| applies_to(&cb, &m.fields.device_id))
        .unwrap_or_else(|| dummy_message(cb.input.downlink));
    let device_name = store
        .get_device(&message.fields.device_id)?
        .map(|d| d.name)
        .unwrap_or_default();
    let timeout = store.settings()?.downlink_timeout_ms;
    let (message, result) = tokio::task::spawn_blocking(move || {
        let result = run_callback(&cb, &message, &device_name, timeout);
        (message, result)
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    to_json(TestResult { message, result })
}

async fn callback_logs(State(host): Shared, Query(q): Query<HashMap<String, String>>) -> ApiResult {
    let logs = host
        .engine
        .store()
        .list_logs(number(&q, "callback"), number(&q, "message"), number(&q, "limit"))?;
    to_json(logs)
}

// The host's log, oldest first
async fn logs(State(host): Shared) -> ApiResult {
    to_json(host.engine.logs())
}

fn sse_event(e: &Event) -> sse::Event {
    sse::Event::default()
        .event(e.name())
        .data(serde_json::to_string(e).unwrap_or_default())
}

// The radio's state first, then everything that happens
async fn events(State(host): Shared) -> Sse<impl Stream<Item = Result<sse::Event, Infallible>>> {
    let live = BroadcastStream::new(host.engine.subscribe()).filter_map(|e| e.ok());
    let first = tokio_stream::once(Event::Radio {
        status: host.radio.status(),
    });
    Sse::new(first.chain(live).map(|e| Ok(sse_event(&e)))).keep_alive(KeepAlive::new().interval(SSE_PING).text("ping"))
}
