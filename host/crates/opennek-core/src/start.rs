//! Starting and stopping the host, the same for every front end: lock the data dir, open the store, listen, start
//! the radio, and stop the radio on the way out

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::engine::Event;
use crate::host::Host;
use crate::instance::{InstanceLock, LockError};
use crate::platform::{Os, default_data_dir};
use crate::radio::Opener;
use crate::serial::SerialOpener;
use crate::store::Store;

/// What a front end decides about the host; the rest comes from the saved settings
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// None for the OS default (see `platform::default_data_dir`)
    pub data_dir: Option<PathBuf>,
    /// Address to listen on
    pub host: String,
    /// 0 picks a free port; `Running::url` says which
    pub port: u16,
    /// Saved as the sigfoxPort setting; "off" saves no port
    pub sigfox_port: Option<String>,
    /// Saved as the sigfoxFreq setting
    pub sigfox_freq: Option<u32>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            data_dir: None,
            host: "127.0.0.1".into(),
            port: 8085,
            sigfox_port: None,
            sigfox_freq: None,
        }
    }
}

#[derive(Debug)]
pub enum StartError {
    /// Another host runs on the data dir; its URL if it has written it
    AlreadyRunning {
        url: Option<String>,
    },
    /// The data directory or the database in it cannot be opened
    DataDir {
        dir: PathBuf,
        error: String,
    },
    Settings(String),
    Listen {
        addr: String,
        error: io::Error,
    },
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartError::AlreadyRunning { url: Some(url) } => write!(f, "OpenNEK is already running at {url}"),
            StartError::AlreadyRunning { url: None } => f.write_str("OpenNEK is already running"),
            StartError::DataDir { dir, error } => write!(f, "{}: {error}", dir.display()),
            StartError::Settings(e) => f.write_str(e),
            StartError::Listen { addr, error } => write!(f, "cannot listen on {addr}: {error}"),
        }
    }
}

impl std::error::Error for StartError {}

/// A started host: the server is listening and the radio is running
pub struct Running {
    /// The web UI, with the port actually bound
    pub url: String,
    pub data_dir: PathBuf,
    pub host: Arc<Host>,
    /// Subscribed before the radio started, so no event is missed
    events: broadcast::Receiver<Event>,
    server: JoinHandle<io::Result<()>>,
    lock: InstanceLock,
}

impl Running {
    /// The events since just before the radio started; a later call gets only the events after it
    pub fn take_events(&mut self) -> broadcast::Receiver<Event> {
        let newer = self.events.resubscribe();
        std::mem::replace(&mut self.events, newer)
    }

    /// Returns only if the server stops by itself, which is an error
    pub async fn stopped(&mut self) -> io::Result<()> {
        (&mut self.server).await.unwrap_or_else(|e| Err(io::Error::other(e)))
    }

    /// Stops the server and the radio, and returns once the radio has let go of the port
    pub async fn shutdown(self) {
        // Event streams stay open, so the server is dropped instead of waiting for them to close
        self.server.abort();
        let host = self.host;
        let _ = tokio::task::spawn_blocking(move || host.radio.stop()).await;
        self.lock.release();
    }
}

/// Starts the host on the USB receiver given by the settings
pub async fn start(options: Options) -> Result<Running, StartError> {
    start_with(options, Box::new(SerialOpener)).await
}

/// Starts the host with the radio opening its port through `opener`
pub async fn start_with(options: Options, opener: Box<dyn Opener>) -> Result<Running, StartError> {
    let dir = options.data_dir.clone().unwrap_or_else(|| {
        let home = std::env::home_dir().unwrap_or_default();
        default_data_dir(Os::current(), |k| std::env::var(k).ok(), &home.to_string_lossy())
    });
    let data_dir_error = |error: String| StartError::DataDir {
        dir: dir.clone(),
        error,
    };
    std::fs::create_dir_all(&dir).map_err(|e| data_dir_error(e.to_string()))?;
    // before the store and the radio, so a second host touches neither
    let lock = InstanceLock::acquire(&dir).map_err(|e| match e {
        LockError::Held { url } => StartError::AlreadyRunning { url },
        LockError::Io(e) => data_dir_error(e.to_string()),
    })?;
    let store = open_store(&dir, &options)?;
    let host = Host::new(store, opener);
    let events = host.engine.subscribe();

    let addr = format!("{}:{}", options.host, options.port);
    let listener = tokio::net::TcpListener::bind((options.host.as_str(), options.port))
        .await
        .map_err(|error| StartError::Listen { addr, error })?;
    let port = listener.local_addr().map_or(options.port, |a| a.port());
    let shown = if options.host == "0.0.0.0" {
        "localhost"
    } else {
        &options.host
    };
    let url = format!("http://{shown}:{port}/");
    lock.publish(&url).map_err(|e| data_dir_error(e.to_string()))?;

    host.radio.start();
    let server = tokio::spawn(axum::serve(listener, crate::server::router(host.clone())).into_future());
    Ok(Running {
        url,
        data_dir: dir,
        host,
        events,
        server,
        lock,
    })
}

fn open_store(dir: &Path, options: &Options) -> Result<Store, StartError> {
    let data_dir_error = |error: String| StartError::DataDir {
        dir: dir.to_path_buf(),
        error,
    };
    let store = Store::open(&dir.join("opennek.db")).map_err(|e| data_dir_error(e.to_string()))?;
    let mut patch = serde_json::Map::new();
    if let Some(port) = &options.sigfox_port {
        patch.insert("sigfoxPort".into(), json!(if port == "off" { "" } else { port }));
    }
    if let Some(freq) = options.sigfox_freq {
        patch.insert("sigfoxFreq".into(), json!(freq));
    }
    if !patch.is_empty() {
        let settings = store
            .settings()
            .map_err(|e| StartError::Settings(e.to_string()))?
            .merged(patch.into())
            .map_err(StartError::Settings)?;
        store
            .save_settings(&settings)
            .map_err(|e| StartError::Settings(e.to_string()))?;
    }
    Ok(store)
}
