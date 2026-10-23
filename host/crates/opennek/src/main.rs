use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use opennek::cli::Args;
use opennek::engine::Event;
use opennek::host::Host;
use opennek::platform::{Os, browser_command, default_data_dir};
use opennek::serial::SerialOpener;
use opennek::store::{DownlinkStatus, Store};
use serde_json::json;

fn ts() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

// One line per event on the console; radio status lines only when they change (not for every frame counted)
fn print_events(host: &Host) {
    let mut events = host.engine.subscribe();
    tokio::spawn(async move {
        let mut last_radio = String::new();
        while let Ok(e) = events.recv().await {
            match e {
                Event::Console { line, .. } => println!("[{}] {line}", ts()),
                // the UI shows callback logs; the console does not
                Event::CallbackLog { .. } => {}
                Event::Radio { status: s } => {
                    let line = format!(
                        "sigfox: {}{}{}{}{}",
                        json!(s.state).as_str().unwrap_or_default(),
                        s.port.as_ref().map(|p| format!(" {p}")).unwrap_or_default(),
                        s.fw.as_ref().map(|f| format!(" fw={f}")).unwrap_or_default(),
                        s.freq
                            .map(|f| format!(" {:.3} MHz", f64::from(f) / 1e6))
                            .unwrap_or_default(),
                        s.error.as_ref().map(|e| format!(" ({e})")).unwrap_or_default(),
                    );
                    if line != last_radio {
                        println!("[{}] {line}", ts());
                    }
                    last_radio = line;
                }
                // a message is reported when received and again when its downlink is settled
                Event::Message { message } => {
                    let m = &message.fields;
                    if matches!(m.downlink_status, DownlinkStatus::None | DownlinkStatus::Pending) {
                        let data = if m.data.is_empty() { "-" } else { &m.data };
                        println!(
                            "[{}] uplink {} seq={} data={data} rssi={} frames={} auth={}{}",
                            ts(),
                            m.device_id,
                            m.seq,
                            m.rssi,
                            m.nb_frames,
                            json!(m.auth).as_str().unwrap_or_default(),
                            if m.ack { " (downlink requested)" } else { "" }
                        );
                    } else {
                        println!(
                            "[{}] downlink {} seq={} {}{}",
                            ts(),
                            m.device_id,
                            m.seq,
                            json!(m.downlink_status).as_str().unwrap_or_default(),
                            m.downlink.as_ref().map(|d| format!(" data={d}")).unwrap_or_default()
                        );
                    }
                }
            }
        }
    });
}

fn open_store(args: &Args) -> Result<(Store, PathBuf), String> {
    let home = std::env::home_dir().unwrap_or_default();
    let dir = args
        .data
        .clone()
        .unwrap_or_else(|| default_data_dir(Os::current(), |k| std::env::var(k).ok(), &home.to_string_lossy()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let store = Store::open(&dir.join("opennek.db")).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut patch = serde_json::Map::new();
    if let Some(port) = &args.sigfox {
        patch.insert("sigfoxPort".into(), json!(if port == "off" { "" } else { port }));
    }
    if let Some(freq) = args.sigfox_freq {
        patch.insert("sigfoxFreq".into(), json!(freq));
    }
    if !patch.is_empty() {
        let settings = store.settings().map_err(|e| e.to_string())?.merged(patch.into())?;
        store.save_settings(&settings).map_err(|e| e.to_string())?;
    }
    Ok((store, dir))
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    let (store, dir) = match open_store(&args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("opennek: {e}");
            return ExitCode::FAILURE;
        }
    };
    let host = Host::new(store, Box::new(SerialOpener));
    print_events(&host);

    let listener = match tokio::net::TcpListener::bind((args.host.as_str(), args.port)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("opennek: cannot listen on {}:{}: {e}", args.host, args.port);
            return ExitCode::FAILURE;
        }
    };
    let shown = if args.host == "0.0.0.0" {
        "localhost"
    } else {
        &args.host
    };
    let url = format!("http://{shown}:{}/", args.port);
    println!("OpenNEK {} — {url}  data: {}", env!("CARGO_PKG_VERSION"), dir.display());
    if args.opens_browser() {
        let (cmd, cmd_args) = browser_command(Os::current(), &url);
        // a missing browser is not worth failing over; the URL is printed above
        let _ = std::process::Command::new(cmd).args(cmd_args).spawn();
    }
    host.radio.start();

    let server = axum::serve(listener, opennek::server::router(host.clone()));
    // Event streams stay open, so the server is dropped instead of waiting for them to close
    tokio::select! {
        r = server => if let Err(e) = r { eprintln!("opennek: {e}") },
        _ = tokio::signal::ctrl_c() => {},
    }
    let radio_host = Arc::clone(&host);
    let _ = tokio::task::spawn_blocking(move || radio_host.radio.stop()).await;
    ExitCode::SUCCESS
}
