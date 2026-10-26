//! Every receiver the host opens by its port path: the RSSI streamers on a serial port and the RTL-SDRs

use std::io;

use crate::radio::{LinePort, Opener};
use crate::rtlsdr::{self, is_rtlsdr};
use crate::serial::{PortInfo, SerialOpener, list_serial_ports};

/// The serial ports, then the RTL-SDRs
pub fn list_ports() -> Vec<PortInfo> {
    let mut ports = list_serial_ports();
    ports.extend(rtlsdr::list_ports());
    ports
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
