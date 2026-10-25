//! `opennek install` on Linux: GNOME Files does not run executables on a double click, so the host goes into the app
//! menu instead. The menu entry runs it in a terminal, which shows the logs and stops the host when closed.

use std::io;
use std::path::{Path, PathBuf};

const ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><circle cx="16" cy="16" r="5" fill="#28a"/><path d="M8 8a11 11 0 0 0 0 16M24 8a11 11 0 0 1 0 16" stroke="#28a" stroke-width="3" fill="none" stroke-linecap="round"/></svg>
"##;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallPaths {
    /// The installed copy of the executable
    pub bin: PathBuf,
    pub desktop: PathBuf,
    pub icon: PathBuf,
}

impl InstallPaths {
    /// Where a desktop looks for a user's apps: ~/.local/bin and $XDG_DATA_HOME (~/.local/share)
    pub fn new(home: &Path, xdg_data_home: Option<&str>) -> InstallPaths {
        let data = xdg_data_home
            .filter(|d| !d.is_empty())
            .map_or_else(|| home.join(".local/share"), PathBuf::from);
        InstallPaths {
            bin: home.join(".local/bin/opennek"),
            desktop: data.join("applications/opennek.desktop"),
            icon: data.join("icons/hicolor/scalable/apps/opennek.svg"),
        }
    }
}

/// Absolute paths, so the entry works whether or not ~/.local/bin is on PATH; Exec is quoted for spaces
pub fn desktop_entry(p: &InstallPaths) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=OpenNEK (Terminal)\nComment=Local network for real Sigfox devices\n\
         Exec=\"{}\"\nIcon={}\nTerminal=true\nCategories=Development;Network;\n",
        p.bin.display(),
        p.icon.display()
    )
}

fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Written next to it and renamed: overwriting the executable of a running host fails ("Text file busy")
    let tmp = path.with_extension("new");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

/// Copies `exe` (the running executable) to the user's bin directory and adds the menu entry and its icon
pub fn install(exe: &Path, p: &InstallPaths) -> io::Result<()> {
    let same = exe
        .canonicalize()
        .ok()
        .zip(p.bin.canonicalize().ok())
        .is_some_and(|(a, b)| a == b);
    if !same {
        write(&p.bin, &std::fs::read(exe)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p.bin, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    write(&p.icon, ICON.as_bytes())?;
    write(&p.desktop, desktop_entry(p).as_bytes())
}

/// Removes what install wrote and returns which of them were there; the data directory stays
pub fn uninstall(p: &InstallPaths) -> io::Result<Vec<PathBuf>> {
    let mut removed = vec![];
    for path in [&p.bin, &p.desktop, &p.icon] {
        match std::fs::remove_file(path) {
            Ok(()) => removed.push(path.clone()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(removed)
}
