//! Every receiver the host opens by its port path: the RSSI streamers on a serial port and the RTL-SDRs

use std::io;

use crate::radio::{LinePort, Opener};
use crate::rtlsdr::{self, is_rtlsdr};
use crate::serial::{PortInfo, SerialOpener, list_serial_ports};
use crate::store::ReceiverPort;

/// The serial ports, then the RTL-SDRs
pub fn list_ports() -> Vec<PortInfo> {
    let mut ports = list_serial_ports();
    ports.extend(rtlsdr::list_ports());
    ports
}

/// The port to take as the receiver's while none was ever set: the XIAO kit, when it is the only one present,
/// else the one RTL-SDR if no kit is (it only receives). Which of several is the user's to say
pub fn auto_port(ports: &[PortInfo], setting: &ReceiverPort) -> Option<String> {
    if *setting != ReceiverPort::Unset {
        return None;
    }
    if ports.iter().any(|p| p.kit) {
        the_one(ports, |p| p.kit)
    } else {
        the_one(ports, |p| is_rtlsdr(&p.path))
    }
}

fn the_one(ports: &[PortInfo], pick: impl Fn(&PortInfo) -> bool) -> Option<String> {
    match ports.iter().filter(|p| pick(p)).collect::<Vec<_>>().as_slice() {
        [p] => Some(p.path.clone()),
        _ => None,
    }
}

pub struct ReceiverOpener;

impl Opener for ReceiverOpener {
    fn ports(&self) -> Vec<PortInfo> {
        list_ports()
    }

    fn open(&self, path: &str) -> io::Result<Box<dyn LinePort>> {
        if is_rtlsdr(path) {
            rtlsdr::open(path)
        } else {
            SerialOpener.open(path)
        }
    }
}
