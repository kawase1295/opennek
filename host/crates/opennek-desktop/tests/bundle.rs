//! The installers' settings. Some are fixed for good: changing them after the first release breaks updates for
//! installed users (the identifier, a per-user NSIS install)

use std::path::{Path, PathBuf};

use serde_json::Value;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn config(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir().join(name)).unwrap()).unwrap()
}

fn read(path: &Value) -> String {
    std::fs::read_to_string(dir().join(path.as_str().unwrap())).unwrap()
}

const IDENTIFIER: &str = "io.github.kawase1295.opennek";

#[test]
fn the_app_is_named_opennek_with_a_fixed_identifier() {
    let c = config("tauri.conf.json");
    assert_eq!(c["productName"], "OpenNEK");
    assert_eq!(c["identifier"], IDENTIFIER);
    assert_eq!(c["mainBinaryName"], "opennek-desktop");
    // the version is the workspace's, from Cargo.toml
    assert!(c.get("version").is_none());
}

#[test]
fn it_bundles_a_deb_and_an_nsis_installer_with_the_icons() {
    let b = &config("tauri.conf.json")["bundle"];
    assert_eq!(b["active"], true);
    assert_eq!(b["targets"], serde_json::json!(["deb", "nsis"]));
    for icon in b["icon"].as_array().unwrap() {
        assert!(dir().join(icon.as_str().unwrap()).exists(), "{icon}");
    }
}

#[test]
fn nsis_installs_per_user_and_fetches_webview2_only_when_missing() {
    let w = &config("tauri.conf.json")["bundle"]["windows"];
    assert_eq!(w["nsis"]["installMode"], "currentUser");
    assert_eq!(w["webviewInstallMode"]["type"], "downloadBootstrapper");
    // signing is left to configuration (a certificate or signCommand from CI secrets)
    assert!(w.get("certificateThumbprint").is_none() && w.get("signCommand").is_none());
}

#[test]
fn on_linux_the_menu_entry_is_named_after_the_identifier_and_shows_opennek() {
    // the .deb names its menu entry after productName, so Linux overrides it; the entry still says OpenNEK
    let c = config("tauri.linux.conf.json");
    assert_eq!(c["productName"], IDENTIFIER);
    let template = read(&c["bundle"]["linux"]["deb"]["desktopTemplate"]);
    assert!(template.contains("\nName=OpenNEK\n"), "{template}");
    assert!(template.contains("\nExec={{exec}}\n") && template.contains("\nTerminal=false\n"));
}

#[test]
fn the_deb_targets_ubuntu_24_04_and_later() {
    let depends = config("tauri.linux.conf.json")["bundle"]["linux"]["deb"]["depends"].clone();
    let depends: Vec<&str> = depends
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d.as_str().unwrap())
        .collect();
    // the versions Ubuntu 24.04 ships; the app links glibc and webkit2gtk dynamically
    for d in [
        "libc6 (>= 2.39)",
        "libwebkit2gtk-4.1-0 (>= 2.44)",
        "libgtk-3-0 (>= 3.24.41)",
    ] {
        assert!(depends.contains(&d), "{depends:?}");
    }
}

#[test]
fn the_deb_lets_the_logged_in_user_open_the_radio() {
    let deb = &config("tauri.linux.conf.json")["bundle"]["linux"]["deb"];
    let rule = read(&deb["files"]["/usr/lib/udev/rules.d/70-opennek.rules"]);
    // the XIAO ESP32S3's USB-Serial/JTAG; uaccess hands it to the user at the seat (needs a rule before 73)
    assert!(
        rule.lines()
            .any(|l| l == r#"SUBSYSTEM=="tty", ATTRS{idVendor}=="303a", ATTRS{idProduct}=="1001", TAG+="uaccess""#),
        "{rule}"
    );
    // reloaded on install and on removal, so it applies without a new login
    for script in ["postInstallScript", "postRemoveScript"] {
        let s = read(&deb[script]);
        assert!(s.starts_with("#!/bin/sh\n"), "{script}");
        assert!(
            s.contains("udevadm control --reload-rules") && s.contains("udevadm trigger"),
            "{script}: {s}"
        );
    }
}

#[test]
fn the_packaging_files_exist() {
    let deb = &config("tauri.linux.conf.json")["bundle"]["linux"]["deb"];
    for p in [
        &deb["desktopTemplate"],
        &deb["postInstallScript"],
        &deb["postRemoveScript"],
    ] {
        assert!(Path::new(&dir().join(p.as_str().unwrap())).is_file(), "{p}");
    }
}
