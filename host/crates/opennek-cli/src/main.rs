use std::process::ExitCode;

use clap::Parser;
use opennek_cli::cli::{Args, Command};
use opennek_cli::install::{InstallPaths, install, uninstall};
use opennek_core::engine::Event;
use opennek_core::platform::{Os, browser_command};
use opennek_core::store::DownlinkStatus;
use opennek_core::{Options, start};
use serde_json::json;
use tokio::sync::broadcast;

fn ts() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

// One line per event on the console; radio status lines only when they change (not for every frame counted)
fn print_events(mut events: broadcast::Receiver<Event>) {
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

// install / uninstall: the app menu entry on Linux
fn manage_install(command: &Command) -> ExitCode {
    if Os::current() != Os::Linux {
        eprintln!("opennek: install and uninstall are for Linux; on Windows, double-click the .exe");
        return ExitCode::FAILURE;
    }
    let home = std::env::home_dir().unwrap_or_default();
    let paths = InstallPaths::new(&home, std::env::var("XDG_DATA_HOME").ok().as_deref());
    let result = match command {
        Command::Install => std::env::current_exe().and_then(|exe| install(&exe, &paths)).map(|_| {
            println!("Installed {}", paths.bin.display());
            println!("Added OpenNEK to the app menu ({})", paths.desktop.display());
        }),
        Command::Uninstall => uninstall(&paths).map(|removed| {
            for path in &removed {
                println!("Removed {}", path.display());
            }
            if removed.is_empty() {
                println!("OpenNEK was not installed");
            }
        }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("opennek: {e}");
            ExitCode::FAILURE
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    if let Some(command) = &args.command {
        return manage_install(command);
    }
    let options = Options {
        data_dir: args.data.clone(),
        host: args.host.clone(),
        port: args.port,
        sigfox_port: args.sigfox.clone(),
        sigfox_freq: args.sigfox_freq,
    };
    let mut running = match start(options).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("opennek: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "OpenNEK {} — {}  data: {}",
        env!("CARGO_PKG_VERSION"),
        running.url,
        running.data_dir.display()
    );
    // the events wait for the line above, so it comes first
    print_events(running.take_events());
    if args.opens_browser() {
        let (cmd, cmd_args) = browser_command(Os::current(), &running.url);
        // a missing browser is not worth failing over; the URL is printed above
        let _ = std::process::Command::new(cmd).args(cmd_args).spawn();
    }

    tokio::select! {
        r = running.stopped() => if let Err(e) = r { eprintln!("opennek: {e}") },
        _ = tokio::signal::ctrl_c() => {},
    }
    running.shutdown().await;
    ExitCode::SUCCESS
}
