use std::io;
use std::path::{Path, PathBuf};

use opennek_core::StartError;
use opennek_desktop::{
    Dialog, PORT, download_name, download_path, failure_dialog, fallback_port, listening_line, panic_reason,
    window_dialog,
};

fn listen_error(kind: io::ErrorKind) -> StartError {
    StartError::Listen {
        addr: format!("127.0.0.1:{PORT}"),
        error: io::Error::from(kind),
    }
}

#[test]
fn listens_on_8085_else_on_a_free_port() {
    assert_eq!(PORT, 8085);
    assert_eq!(fallback_port(PORT, &listen_error(io::ErrorKind::AddrInUse)), Some(0));
    // Windows refuses ports in its reserved ranges with "access denied" rather than "in use"
    assert_eq!(
        fallback_port(PORT, &listen_error(io::ErrorKind::PermissionDenied)),
        Some(0)
    );
    // a free port that fails too is not tried again
    assert_eq!(fallback_port(0, &listen_error(io::ErrorKind::AddrInUse)), None);
    // another OpenNEK holds the data dir: its port is not ours to take
    let running = StartError::AlreadyRunning {
        url: Some("http://127.0.0.1:8085/".into()),
    };
    assert_eq!(fallback_port(PORT, &running), None);
    assert_eq!(fallback_port(PORT, &StartError::Settings("bad".into())), None);
}

#[test]
fn the_log_says_which_port_was_chosen() {
    assert_eq!(
        listening_line("http://127.0.0.1:8085/", false),
        "desktop: listening on http://127.0.0.1:8085/"
    );
    assert_eq!(
        listening_line("http://127.0.0.1:41234/", true),
        "desktop: port 8085 is taken by other software, listening on http://127.0.0.1:41234/"
    );
}

#[test]
fn a_second_launch_says_where_opennek_runs() {
    let e = StartError::AlreadyRunning {
        url: Some("http://127.0.0.1:8085/".into()),
    };
    assert_eq!(
        failure_dialog(&e, Path::new("/d/logs/opennek.log")),
        Dialog {
            title: "OpenNEK is already running".into(),
            text: "OpenNEK is already running at http://127.0.0.1:8085/".into(),
            error: false,
        }
    );
    // it may not have written its URL yet
    let e = StartError::AlreadyRunning { url: None };
    assert_eq!(
        failure_dialog(&e, Path::new("/d/logs/opennek.log")).text,
        "OpenNEK is already running"
    );
}

#[test]
fn a_start_up_failure_says_what_happened_and_where_the_log_is() {
    let e = StartError::DataDir {
        dir: PathBuf::from("/d"),
        error: "permission denied".into(),
    };
    let log = Path::new("/d/logs/opennek.log");
    assert_eq!(
        failure_dialog(&e, log),
        Dialog {
            title: "OpenNEK could not start".into(),
            text: format!("/d: permission denied\n\nLog: {}", log.display()),
            error: true,
        }
    );
}

#[test]
fn downloads_keep_their_name_and_do_not_replace_a_file() {
    let dir = Path::new("/home/u/Downloads");
    let taken = [dir.join("opennek-messages.csv"), dir.join("opennek-messages (1).csv")];
    let exists = |p: &Path| taken.iter().any(|t| t == p);
    assert_eq!(download_path(dir, "new.csv", exists), dir.join("new.csv"));
    assert_eq!(
        download_path(dir, "opennek-messages.csv", exists),
        dir.join("opennek-messages (2).csv")
    );
    // without an extension the number goes at the end
    let taken = [dir.join("data")];
    assert_eq!(
        download_path(dir, "data", |p: &Path| taken.iter().any(|t| t == p)),
        dir.join("data (1)")
    );
}

#[test]
fn the_download_name_comes_from_the_webview_else_the_url() {
    let url = url::Url::parse("http://127.0.0.1:8085/api/messages.csv?device=1").unwrap();
    // the webview suggests the name from Content-Disposition
    let suggested = Path::new("/tmp/opennek-messages.csv");
    assert_eq!(download_name(&url, Some(suggested)), "opennek-messages.csv");
    assert_eq!(download_name(&url, None), "messages.csv");
    assert_eq!(download_name(&url, Some(Path::new(""))), "messages.csv");
    let root = url::Url::parse("http://127.0.0.1:8085/").unwrap();
    assert_eq!(download_name(&root, None), "download");
    // never a path out of the downloads folder
    assert_eq!(download_name(&url, Some(Path::new("../../.bashrc"))), ".bashrc");
}

#[test]
fn a_window_that_cannot_open_says_why_and_where_the_log_is() {
    let log = Path::new("/d/logs/opennek.log");
    assert_eq!(
        window_dialog("Failed to initialize GTK", log),
        Dialog {
            title: "OpenNEK could not start".into(),
            text: format!(
                "The window cannot be opened: Failed to initialize GTK\n\nLog: {}",
                log.display()
            ),
            error: true,
        }
    );
}

#[test]
fn a_panic_gives_its_message() {
    // the webview libraries panic instead of returning an error (no display on Linux)
    let s = std::panic::catch_unwind(|| panic!("no display {}", 1)).unwrap_err();
    assert_eq!(panic_reason(s.as_ref()), "no display 1");
    let s = std::panic::catch_unwind(|| std::panic::panic_any("static")).unwrap_err();
    assert_eq!(panic_reason(s.as_ref()), "static");
    let s = std::panic::catch_unwind(|| std::panic::panic_any(42)).unwrap_err();
    assert_eq!(panic_reason(s.as_ref()), "unknown error");
}
