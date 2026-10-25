// No console window next to the app on Windows; debug builds keep it for their output
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use opennek_core::host::Host;
use opennek_core::logfile::{LIMIT, LogFile, log_path};
use opennek_core::radio::now_ms;
use opennek_core::{Options, Running, StartError, start};
use opennek_desktop::{
    Dialog, PORT, download_name, download_path, failure_dialog, fallback_port, listening_line, panic_reason,
    window_dialog,
};
use tauri::webview::DownloadEvent;
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

/// Starts the host on 127.0.0.1 only (the API has no authentication and can make the radio transmit), on 8085 or a
/// free port when other software has it
async fn start_host() -> Result<Running, (StartError, PathBuf)> {
    let mut options = Options {
        port: PORT,
        ..Options::default()
    };
    loop {
        match start(options.clone()).await {
            Ok(running) => {
                let line = listening_line(&running.url, options.port != PORT);
                running.host.engine.console(line);
                return Ok(running);
            }
            Err(e) => match fallback_port(options.port, &e) {
                Some(port) => options.port = port,
                None => return Err((e, options.resolved_data_dir())),
            },
        }
    }
}

fn show(dialog: &Dialog) {
    // also for a terminal, and for Linux without a display, where the message box would wait forever
    eprintln!("opennek-desktop: {}", dialog.text);
    if cfg!(target_os = "linux")
        && std::env::var_os("DISPLAY").is_none()
        && std::env::var_os("WAYLAND_DISPLAY").is_none()
    {
        return;
    }
    let level = if dialog.error {
        rfd::MessageLevel::Error
    } else {
        rfd::MessageLevel::Info
    };
    rfd::MessageDialog::new()
        .set_title(&dialog.title)
        .set_description(&dialog.text)
        .set_level(level)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

/// CSV and other `<a download>` links save to the downloads folder without asking, and the log says where
fn on_download(host: Arc<Host>, dir: PathBuf) -> impl Fn(tauri::Webview, DownloadEvent<'_>) -> bool {
    move |_, event| {
        match event {
            DownloadEvent::Requested { url, destination } => {
                let name = download_name(&url, Some(destination.as_path()));
                *destination = download_path(&dir, &name, |p| p.exists());
            }
            DownloadEvent::Finished { url, path, success } => {
                let line = match (success, path) {
                    (true, Some(path)) => format!("desktop: saved {}", path.display()),
                    (true, None) => format!("desktop: saved {url}"),
                    (false, _) => format!("desktop: download failed: {url}"),
                };
                host.engine.console(line);
            }
            _ => {}
        }
        true
    }
}

fn main() {
    let running = match tauri::async_runtime::block_on(start_host()) {
        Ok(running) => running,
        Err((e, dir)) => {
            let log = log_path(&dir);
            // the dialog points at the log, so the failure is in it (another OpenNEK owns the log it would go to)
            if !matches!(e, StartError::AlreadyRunning { .. }) {
                LogFile::new(log.clone(), LIMIT).write(
                    now_ms(),
                    &format!("OpenNEK {} could not start: {e}", env!("CARGO_PKG_VERSION")),
                );
            }
            show(&failure_dialog(&e, &log));
            std::process::exit(1);
        }
    };
    let url: tauri::Url = running.url.parse().expect("the host's URL");
    let host = running.host.clone();
    let log = running.log_file.clone();
    let running = Arc::new(Mutex::new(Some(running)));

    let builder = tauri::Builder::default().setup(move |app| {
        let downloads = app
            .path()
            .download_dir()
            .or_else(|_| app.path().home_dir())
            .unwrap_or_default();
        // created once the server listens; the page is the host's, so it gets no Tauri commands
        WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
            .title("OpenNEK")
            .inner_size(1200.0, 800.0)
            .min_inner_size(480.0, 360.0)
            .on_download(on_download(host, downloads))
            .build()?;
        Ok(())
    });
    // Without a display the webview libraries panic rather than return an error; either way the host is stopped
    // and the reason logged
    let app = std::panic::catch_unwind(AssertUnwindSafe(|| builder.build(tauri::generate_context!())))
        .map_err(|p| panic_reason(p.as_ref()))
        .and_then(|r| r.map_err(|e| e.to_string()));
    let app = match app {
        Ok(app) => app,
        Err(reason) => {
            if let Some(r) = running.lock().unwrap_or_else(|e| e.into_inner()).take() {
                r.host
                    .engine
                    .console(format!("desktop: the window cannot be opened: {reason}"));
                tauri::async_runtime::block_on(r.shutdown());
            }
            show(&window_dialog(&reason, &log));
            std::process::exit(1);
        }
    };
    // Closing the window quits the app: stop the radio and release the data dir before the process ends
    app.run(move |_, event| {
        if let RunEvent::Exit = event
            && let Some(r) = running.lock().unwrap_or_else(|e| e.into_inner()).take()
        {
            tauri::async_runtime::block_on(r.shutdown());
        }
    });
}
