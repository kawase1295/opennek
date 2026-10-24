mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use common::{FakeStreamer, eventually};
use opennek_core::engine::Event;
use opennek_core::radio::RadioState;
use opennek_core::store::Store;
use opennek_core::{Options, StartError, start_with};
use serde_json::Value;

const STATUS: &str = "I fw=0.3.0 f=923200000 bw=10 sr=2000 boost=1 cfo=0 pd=4";

fn options(data: &std::path::Path) -> Options {
    Options {
        data_dir: Some(data.to_path_buf()),
        port: 0,
        sigfox_port: Some("fake0".into()),
        ..Options::default()
    }
}

// A plain HTTP/1.0 request, so the test needs no HTTP client
fn get(url: &str, path: &str) -> (String, String) {
    let addr = url.trim_start_matches("http://").trim_end_matches('/');
    let mut stream = TcpStream::connect(addr.replace("localhost", "127.0.0.1")).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(stream, "GET {path} HTTP/1.0\r\nHost: {addr}\r\n\r\n").unwrap();
    let mut res = String::new();
    stream.read_to_string(&mut res).unwrap();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    (head.to_string(), body.to_string())
}

#[test]
fn the_defaults_are_the_cli_defaults() {
    let o = Options::default();
    assert_eq!(
        (o.data_dir, o.host.as_str(), o.port, o.sigfox_port, o.sigfox_freq),
        (None, "127.0.0.1", 8085, None, None)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn starts_on_the_port_it_was_given_and_stops_the_radio_on_shutdown() {
    let data = tempfile::tempdir().unwrap();
    let streamer = FakeStreamer::with_lines(vec![STATUS.into()]);
    let running = start_with(options(data.path()), Box::new(streamer.clone()))
        .await
        .unwrap();

    // port 0 picks a free port, and the URL says which
    let port: u16 = running
        .url
        .strip_prefix("http://127.0.0.1:")
        .and_then(|p| p.strip_suffix('/'))
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(port, 0);
    assert_eq!(running.data_dir, data.path());
    assert!(data.path().join("opennek.db").exists());

    let url = running.url.clone();
    let (head, body) = tokio::task::spawn_blocking(move || get(&url, "/api/status"))
        .await
        .unwrap();
    assert!(head.starts_with("HTTP/1.0 200"), "{head}");
    let status: Value = serde_json::from_str(&body).unwrap();
    assert!(status.is_object(), "{body}");

    // the radio was started with the port from the options
    let host = running.host.clone();
    eventually("the radio to connect", || {
        (host.radio.status().state == RadioState::Connected).then_some(())
    });
    assert_eq!(*streamer.opened.lock().unwrap(), ["fake0"]);

    running.shutdown().await;
    // a stopped radio reads no more lines
    streamer.lines.lock().unwrap().push_back(STATUS.into());
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(streamer.lines.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_events_start_before_the_radio() {
    let data = tempfile::tempdir().unwrap();
    let mut running = start_with(options(data.path()), Box::new(FakeStreamer::default()))
        .await
        .unwrap();
    // the first thing the radio says is that it is connecting, and a front end sees it however late it asks
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut events = running.take_events();
    let first = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Event::Radio { status } = events.recv().await.unwrap() {
                return status.state;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(first, RadioState::Connecting);
    running.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sigfox_options_are_saved_in_the_settings() {
    let data = tempfile::tempdir().unwrap();
    let o = Options {
        sigfox_port: Some("off".into()),
        sigfox_freq: Some(920_800_000),
        ..options(data.path())
    };
    start_with(o, Box::new(FakeStreamer::default()))
        .await
        .unwrap()
        .shutdown()
        .await;
    let s = Store::open(&data.path().join("opennek.db"))
        .unwrap()
        .settings()
        .unwrap();
    assert_eq!((s.sigfox_port.as_str(), s.sigfox_freq), ("", 920_800_000));

    // without them the saved settings stay
    let o = Options {
        sigfox_port: None,
        ..options(data.path())
    };
    start_with(o, Box::new(FakeStreamer::default()))
        .await
        .unwrap()
        .shutdown()
        .await;
    let s = Store::open(&data.path().join("opennek.db"))
        .unwrap()
        .settings()
        .unwrap();
    assert_eq!((s.sigfox_port.as_str(), s.sigfox_freq), ("", 920_800_000));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_port_in_use_is_an_error_that_names_it() {
    let data = tempfile::tempdir().unwrap();
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let o = Options {
        port,
        ..options(data.path())
    };
    let e = start_with(o, Box::new(FakeStreamer::default())).await.err().unwrap();
    assert!(matches!(e, StartError::Listen { .. }), "{e}");
    assert!(
        e.to_string()
            .starts_with(&format!("cannot listen on 127.0.0.1:{port}: ")),
        "{e}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn listening_on_all_addresses_shows_localhost() {
    let data = tempfile::tempdir().unwrap();
    let o = Options {
        host: "0.0.0.0".into(),
        ..options(data.path())
    };
    let running = start_with(o, Box::new(FakeStreamer::default())).await.unwrap();
    assert!(running.url.starts_with("http://localhost:"), "{}", running.url);
    running.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_start_on_the_same_data_dir_says_where_the_first_runs() {
    let data = tempfile::tempdir().unwrap();
    let first = start_with(options(data.path()), Box::new(FakeStreamer::default()))
        .await
        .unwrap();

    let second = FakeStreamer::default();
    let o = Options {
        sigfox_freq: Some(920_800_000),
        ..options(data.path())
    };
    let e = start_with(o, Box::new(second.clone())).await.err().unwrap();
    match &e {
        StartError::AlreadyRunning { url } => assert_eq!(url.as_deref(), Some(first.url.as_str())),
        e => panic!("{e}"),
    }
    assert_eq!(e.to_string(), format!("OpenNEK is already running at {}", first.url));

    // it touched neither the settings nor the receiver
    std::thread::sleep(Duration::from_millis(100));
    assert!(second.opened.lock().unwrap().is_empty());
    first.shutdown().await;
    let s = Store::open(&data.path().join("opennek.db"))
        .unwrap()
        .settings()
        .unwrap();
    assert_eq!(s.sigfox_freq, 923_200_000);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_data_dir_is_free_again_after_shutdown() {
    let data = tempfile::tempdir().unwrap();
    for _ in 0..2 {
        start_with(options(data.path()), Box::new(FakeStreamer::default()))
            .await
            .unwrap()
            .shutdown()
            .await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn another_data_dir_is_another_instance() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let first = start_with(options(a.path()), Box::new(FakeStreamer::default()))
        .await
        .unwrap();
    let second = start_with(options(b.path()), Box::new(FakeStreamer::default()))
        .await
        .unwrap();
    assert_ne!(first.url, second.url);
    first.shutdown().await;
    second.shutdown().await;
}
