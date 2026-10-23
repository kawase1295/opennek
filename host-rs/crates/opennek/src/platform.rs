//! OS defaults: where the data lives and how a browser is opened

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
    MacOs,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }

    fn join(self, parts: &[&str]) -> PathBuf {
        let sep = if self == Os::Windows { "\\" } else { "/" };
        PathBuf::from(parts.join(sep))
    }
}

pub fn default_data_dir(os: Os, env: impl Fn(&str) -> Option<String>, home: &str) -> PathBuf {
    let set = |k: &str| env(k).filter(|v| !v.is_empty());
    match os {
        Os::Windows => match set("APPDATA") {
            Some(appdata) => os.join(&[&appdata, "opennek"]),
            None => os.join(&[home, "AppData", "Roaming", "opennek"]),
        },
        Os::MacOs => os.join(&[home, "Library", "Application Support", "opennek"]),
        Os::Linux => match set("XDG_DATA_HOME") {
            Some(data) => os.join(&[&data, "opennek"]),
            None => os.join(&[home, ".local", "share", "opennek"]),
        },
    }
}

pub fn browser_command(os: Os, url: &str) -> (&'static str, Vec<String>) {
    match os {
        Os::Windows => ("cmd", vec!["/c".into(), "start".into(), String::new(), url.into()]),
        Os::MacOs => ("open", vec![url.into()]),
        Os::Linux => ("xdg-open", vec![url.into()]),
    }
}
