use std::path::Path;

use opennek_cli::install::{InstallPaths, desktop_entry, install, uninstall};

fn paths(home: &Path) -> InstallPaths {
    InstallPaths::new(home, None)
}

#[test]
fn files_go_where_the_desktop_looks_for_them() {
    let p = InstallPaths::new(Path::new("/home/u"), None);
    assert_eq!(p.bin, Path::new("/home/u/.local/bin/opennek"));
    assert_eq!(
        p.desktop,
        Path::new("/home/u/.local/share/applications/opennek.desktop")
    );
    assert_eq!(
        p.icon,
        Path::new("/home/u/.local/share/icons/hicolor/scalable/apps/opennek.svg")
    );
    let p = InstallPaths::new(Path::new("/home/u"), Some("/data"));
    assert_eq!(p.desktop, Path::new("/data/applications/opennek.desktop"));
    assert_eq!(p.bin, Path::new("/home/u/.local/bin/opennek"));
}

// The entry is for Linux desktops; elsewhere the paths in it have other separators
#[cfg(unix)]
#[test]
fn the_menu_entry_runs_the_installed_copy_in_a_terminal() {
    let p = InstallPaths::new(Path::new("/home/a user"), None);
    let entry = desktop_entry(&p);
    assert!(entry.starts_with("[Desktop Entry]\n"));
    assert!(entry.contains("\nName=OpenNEK\n"));
    assert!(
        entry.contains("\nExec=\"/home/a user/.local/bin/opennek\"\n"),
        "{entry}"
    );
    assert!(entry.contains("\nIcon=/home/a user/.local/share/icons/hicolor/scalable/apps/opennek.svg\n"));
    // the logs stay visible, and closing the window stops the host
    assert!(entry.contains("\nTerminal=true\n"));
}

#[test]
fn install_copies_the_executable_and_writes_the_entry_and_icon() {
    let home = tempfile::tempdir().unwrap();
    let exe = home.path().join("Downloads/opennek-0.1.0-linux-x64");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"binary v1").unwrap();
    let p = paths(home.path());

    install(&exe, &p).unwrap();
    assert_eq!(std::fs::read(&p.bin).unwrap(), b"binary v1");
    assert_eq!(std::fs::read_to_string(&p.desktop).unwrap(), desktop_entry(&p));
    assert!(std::fs::read_to_string(&p.icon).unwrap().starts_with("<svg"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&p.bin).unwrap().permissions().mode() & 0o777, 0o755);
    }

    // a newer download replaces it; installing from the installed copy keeps it
    std::fs::write(&exe, b"binary v2").unwrap();
    install(&exe, &p).unwrap();
    assert_eq!(std::fs::read(&p.bin).unwrap(), b"binary v2");
    install(&p.bin, &p).unwrap();
    assert_eq!(std::fs::read(&p.bin).unwrap(), b"binary v2");
}

#[test]
fn uninstall_removes_what_install_wrote_and_nothing_else() {
    let home = tempfile::tempdir().unwrap();
    let exe = home.path().join("opennek");
    std::fs::write(&exe, b"binary").unwrap();
    let p = paths(home.path());
    let data = home.path().join(".local/share/opennek/opennek.db");
    std::fs::create_dir_all(data.parent().unwrap()).unwrap();
    std::fs::write(&data, b"db").unwrap();

    install(&exe, &p).unwrap();
    let removed = uninstall(&p).unwrap();
    assert_eq!(removed, [p.bin.clone(), p.desktop.clone(), p.icon.clone()]);
    assert!(!p.bin.exists() && !p.desktop.exists() && !p.icon.exists());
    assert!(data.exists());
    assert!(uninstall(&p).unwrap().is_empty());
}
