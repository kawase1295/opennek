mod common;

use std::sync::Arc;

use common::{FakeStreamer, eventually};
use opennek_core::host::Host;
use opennek_core::serial::PortInfo;
use opennek_core::store::{ReceiverPort, Store};

fn kit(path: &str) -> PortInfo {
    PortInfo {
        path: path.into(),
        label: "XIAO ESP32S3 kit".into(),
        kit: true,
    }
}

fn host(port: ReceiverPort, streamer: &FakeStreamer) -> Arc<Host> {
    let store = Store::open_in_memory().unwrap();
    let mut s = store.settings().unwrap();
    s.sigfox_port = port;
    store.save_settings(&s).unwrap();
    Host::new(store, Box::new(streamer.clone()))
}

fn logs(host: &Host) -> Vec<String> {
    host.engine.logs().into_iter().map(|l| l.line).collect()
}

#[test]
fn the_one_kit_present_is_picked_saved_and_opened() {
    let streamer = FakeStreamer::default();
    *streamer.ports.lock().unwrap() = vec![kit("fake0")];
    let host = host(ReceiverPort::Unset, &streamer);
    host.radio.start();
    eventually("fake0 opened", || {
        (streamer.opened.lock().unwrap().as_slice() == ["fake0"]).then_some(())
    });
    host.radio.stop();
    assert_eq!(
        host.engine.store().settings().unwrap().sigfox_port,
        ReceiverPort::Port("fake0".into())
    );
    assert!(logs(&host).contains(&"sigfox: found the XIAO kit on fake0".to_string()));
}

#[test]
fn an_rtl_sdr_without_a_kit_is_picked_and_said_so() {
    let streamer = FakeStreamer::default();
    *streamer.ports.lock().unwrap() = vec![opennek_core::rtlsdr::port_info(0, "RTL2838UHIDIR", "00000001")];
    let host = host(ReceiverPort::Unset, &streamer);
    host.radio.start();
    eventually("rtlsdr:0 opened", || {
        (streamer.opened.lock().unwrap().as_slice() == ["rtlsdr:0"]).then_some(())
    });
    host.radio.stop();
    assert!(logs(&host).contains(&"sigfox: found an RTL-SDR on rtlsdr:0".to_string()));
}

#[test]
fn a_kit_plugged_in_after_start_is_picked_too() {
    let streamer = FakeStreamer::default();
    let host = host(ReceiverPort::Unset, &streamer);
    host.radio.start();
    eventually("the ports listed", || {
        (*streamer.listed.lock().unwrap() > 0).then_some(())
    });
    *streamer.ports.lock().unwrap() = vec![kit("fake0")];
    eventually("fake0 opened", || {
        (streamer.opened.lock().unwrap().as_slice() == ["fake0"]).then_some(())
    });
    host.radio.stop();
}

#[test]
fn nothing_is_picked_when_none_was_chosen_or_the_kits_are_several() {
    for (port, kits) in [
        (ReceiverPort::Off, vec![kit("fake0")]),
        (ReceiverPort::Unset, vec![kit("fake0"), kit("fake1")]),
    ] {
        let streamer = FakeStreamer::default();
        *streamer.ports.lock().unwrap() = kits;
        let host = host(port.clone(), &streamer);
        host.radio.start();
        if port == ReceiverPort::Unset {
            eventually("the ports listed", || {
                (*streamer.listed.lock().unwrap() > 0).then_some(())
            });
        } else {
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        host.radio.stop();
        assert!(streamer.opened.lock().unwrap().is_empty(), "{port:?}");
        assert_eq!(host.engine.store().settings().unwrap().sigfox_port, port);
    }
}
