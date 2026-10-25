//! The desktop app's decisions, kept apart from Tauri so they are plain functions with tests

use std::path::{Path, PathBuf};

use opennek_core::StartError;

/// The port the app tries first, the CLI's default
pub const PORT: u16 = 8085;

/// The port to try after listening on `tried` failed with `error`, or None to give up
pub fn fallback_port(tried: u16, error: &StartError) -> Option<u16> {
    // any listen error on the default port means other software has it (on Windows a reserved range says "access
    // denied"); 0 lets the OS pick a free one. Another OpenNEK is AlreadyRunning, never a listen error
    (tried == PORT && matches!(error, StartError::Listen { .. })).then_some(0)
}

/// The log line saying where the app listens
pub fn listening_line(url: &str, fell_back: bool) -> String {
    if fell_back {
        format!("desktop: port {PORT} is taken by other software, listening on {url}")
    } else {
        format!("desktop: listening on {url}")
    }
}

/// A native message box shown before the app quits
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialog {
    pub title: String,
    pub text: String,
    /// An error icon; otherwise an information one
    pub error: bool,
}

/// What to tell the user when the host does not start; `log` is the log file in the data dir
pub fn failure_dialog(error: &StartError, log: &Path) -> Dialog {
    match error {
        StartError::AlreadyRunning { .. } => Dialog {
            title: "OpenNEK is already running".into(),
            text: error.to_string(),
            error: false,
        },
        _ => Dialog {
            title: "OpenNEK could not start".into(),
            text: format!("{error}\n\nLog: {}", log.display()),
            error: true,
        },
    }
}

/// What to tell the user when the host started but its window does not open
pub fn window_dialog(reason: &str, log: &Path) -> Dialog {
    Dialog {
        title: "OpenNEK could not start".into(),
        text: format!("The window cannot be opened: {reason}\n\nLog: {}", log.display()),
        error: true,
    }
}

/// The message of a caught panic
pub fn panic_reason(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown error".into())
}

/// The file name of a download: the one the webview suggests (from Content-Disposition), else the URL's last segment
pub fn download_name(url: &url::Url, suggested: Option<&Path>) -> String {
    let from_url = || {
        url.path_segments()
            .and_then(|mut s| s.next_back())
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    // only the name, so a download never lands outside the downloads folder
    suggested
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .or_else(from_url)
        .unwrap_or_else(|| "download".into())
}

/// `name` in `dir`, numbered like a browser does ("name (1).csv") when that file exists
pub fn download_path(dir: &Path, name: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let first = dir.join(name);
    if !exists(&first) {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !exists(p))
        .unwrap_or(first)
}
