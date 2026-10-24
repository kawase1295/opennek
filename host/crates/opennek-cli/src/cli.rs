//! Command-line options

use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand};

/// OpenNEK — a local network for real Sigfox devices
#[derive(Parser)]
#[command(name = "opennek", version, disable_version_flag = true)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,
    /// Port of the web UI / API
    #[arg(long, default_value_t = 8085)]
    pub port: u16,
    /// Address to listen on
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    /// Port of the RSSI streamer that receives real Sigfox (RC3); off to disable [default: the saved setting]
    #[arg(long, value_name = "PATH")]
    pub sigfox: Option<String>,
    /// Sigfox receive frequency in Hz [default: 923200000]
    #[arg(long, value_name = "HZ")]
    pub sigfox_freq: Option<u32>,
    /// Data directory [default: $XDG_DATA_HOME/opennek, %APPDATA%\opennek on Windows, ~/Library/Application
    /// Support/opennek on macOS]
    #[arg(long, value_name = "DIR")]
    pub data: Option<PathBuf>,
    /// Do not open the UI in a browser after starting
    #[arg(long)]
    pub no_open: bool,
    /// Open the UI in a browser after starting (the default; kept for older scripts)
    #[arg(long, hide = true)]
    pub open: bool,
    /// Print the version
    #[arg(short = 'v', long, action = ArgAction::Version)]
    pub version: (),
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Add OpenNEK to the app menu (Linux): copies this executable to ~/.local/bin
    Install,
    /// Remove what install added; the data stays
    Uninstall,
}

impl Args {
    /// Started by a double click or from an app menu there is no way to ask, so the browser opens unless told not to
    pub fn opens_browser(&self) -> bool {
        !self.no_open
    }
}
