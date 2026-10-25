//! The receiver's USB serial port: listing the candidates and reading it line by line

use std::cmp::Ordering;
use std::io::{self, Read, Write};
use std::time::Duration;

use serde::Serialize;

use crate::platform::Os;
use crate::radio::{LinePort, Opener};
use crate::store::ReceiverPort;

const BAUD: u32 = 115_200;
/// Reads return this often, so that a stop or reconnect is noticed
const READ_TIMEOUT: Duration = Duration::from_millis(100);
/// A longer line is garbage (the line speed or the device is wrong)
const MAX_LINE: usize = 4096;
/// The XIAO ESP32S3's USB-Serial/JTAG, as in the .deb's udev rule
const KIT_VID: u16 = 0x303a;
const KIT_PID: u16 = 0x1001;
const KIT_LABEL: &str = "XIAO ESP32S3 kit";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsbInfo {
    pub vid: u16,
    pub pid: u16,
    pub manufacturer: Option<String>,
    pub serial_number: Option<String>,
    /// The device name; on Windows the friendly name ("USB Serial Device (COM4)")
    pub product: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedPort {
    pub path: String,
    pub usb: Option<UsbInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PortInfo {
    pub path: String,
    pub label: String,
    /// The XIAO ESP32S3 & Wio-SX1262 kit
    pub kit: bool,
}

impl UsbInfo {
    fn is_kit(&self) -> bool {
        (self.vid, self.pid) == (KIT_VID, KIT_PID)
    }
}

// Like localeCompare with numeric: COM2 before COM10
fn natural(a: &str, b: &str) -> Ordering {
    let key = |s: &str| {
        let mut parts: Vec<(String, u64)> = vec![];
        let mut text = String::new();
        let mut digits = String::new();
        for c in s.chars().chain([' ']) {
            if c.is_ascii_digit() {
                digits.push(c);
            } else {
                if !digits.is_empty() {
                    parts.push((std::mem::take(&mut text), digits.parse().unwrap_or(u64::MAX)));
                    digits.clear();
                }
                text.push(c);
            }
        }
        parts.push((text, 0));
        parts
    };
    key(a).cmp(&key(b))
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The ports worth offering on each OS: Linux hides the built-in ttyS ports, macOS the ones that are not USB
/// (Bluetooth). Windows keeps every COM port, since a USB serial adapter there may have no other mark.
pub fn to_port_list(ports: Vec<ListedPort>, os: Os) -> Vec<PortInfo> {
    let named = |usb: &UsbInfo| {
        let parts: Vec<&str> = [&usb.manufacturer, &usb.serial_number]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    };
    let mut list: Vec<PortInfo> = match os {
        Os::Windows => ports
            .into_iter()
            .map(|p| {
                let label = p.usb.as_ref().and_then(|u| u.product.clone().or_else(|| named(u)));
                PortInfo {
                    label: label.unwrap_or_else(|| p.path.clone()),
                    kit: is_kit(&p),
                    path: p.path,
                }
            })
            .collect(),
        Os::MacOs => ports
            .into_iter()
            .filter_map(|p| {
                let kit = is_kit(&p);
                let usb = p.usb?;
                // cu is the callout node: it opens without waiting for the other side's DCD
                let path = match p.path.strip_prefix("/dev/tty.") {
                    Some(name) => format!("/dev/cu.{name}"),
                    None => p.path,
                };
                Some(PortInfo {
                    label: named(&usb).unwrap_or_else(|| basename(&path).into()),
                    path,
                    kit,
                })
            })
            .collect(),
        Os::Linux => ports
            .into_iter()
            .filter(|p| p.usb.is_some() || is_usb_tty(&p.path))
            .map(|p| PortInfo {
                label: p
                    .usb
                    .as_ref()
                    .and_then(named)
                    .unwrap_or_else(|| basename(&p.path).into()),
                kit: is_kit(&p),
                path: p.path,
            })
            .collect(),
    };
    for p in list.iter_mut().filter(|p| p.kit) {
        p.label = KIT_LABEL.into();
    }
    list.sort_by(|a, b| natural(&a.path, &b.path));
    list.dedup_by(|a, b| a.path == b.path);
    list
}

fn is_kit(p: &ListedPort) -> bool {
    p.usb.as_ref().is_some_and(UsbInfo::is_kit)
}

/// The port to take as the receiver's while none was ever set: the XIAO kit, when it is the only one present.
/// Which of several kits is the user's to say
pub fn auto_port(ports: &[PortInfo], setting: &ReceiverPort) -> Option<String> {
    if *setting != ReceiverPort::Unset {
        return None;
    }
    match ports.iter().filter(|p| p.kit).collect::<Vec<_>>().as_slice() {
        [kit] => Some(kit.path.clone()),
        _ => None,
    }
}

fn is_usb_tty(path: &str) -> bool {
    let name = basename(path);
    ["ttyACM", "ttyUSB"].iter().any(|prefix| {
        name.strip_prefix(prefix)
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    })
}

pub fn list_serial_ports() -> Vec<PortInfo> {
    let ports = serialport::available_ports().unwrap_or_default();
    let listed = ports
        .into_iter()
        .map(|p| ListedPort {
            path: p.port_name,
            usb: match p.port_type {
                serialport::SerialPortType::UsbPort(u) => Some(UsbInfo {
                    vid: u.vid,
                    pid: u.pid,
                    manufacturer: u.manufacturer,
                    serial_number: u.serial_number,
                    product: u.product,
                }),
                _ => None,
            },
        })
        .collect();
    to_port_list(listed, Os::current())
}

/// Opens serial ports at 115200 8N1
pub struct SerialOpener;

impl Opener for SerialOpener {
    fn ports(&self) -> Vec<PortInfo> {
        list_serial_ports()
    }

    fn open(&self, path: &str) -> io::Result<Box<dyn LinePort>> {
        let port = serialport::new(path, BAUD).timeout(READ_TIMEOUT).open().map_err(|e| {
            let denied = matches!(e.kind, serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied))
                || e.description.to_lowercase().contains("permission denied");
            let hint = if denied && Os::current() == Os::Linux {
                " — run `sudo usermod -aG dialout $USER` and log in again"
            } else {
                ""
            };
            io::Error::other(format!("{}{hint}", e.description))
        })?;
        Ok(Box::new(SerialLinePort { port, buf: vec![] }))
    }
}

struct SerialLinePort {
    port: Box<dyn serialport::SerialPort>,
    buf: Vec<u8>,
}

impl LinePort for SerialLinePort {
    fn read_line(&mut self) -> io::Result<Option<String>> {
        loop {
            if let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.buf.drain(..=i).collect();
                return Ok(Some(String::from_utf8_lossy(&line[..i]).into_owned()));
            }
            let mut chunk = [0u8; 1024];
            match self.port.read(&mut chunk) {
                // a USB serial port that reads nothing without a timeout has been unplugged
                Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "port closed")),
                Ok(n) => {
                    self.buf.extend_from_slice(&chunk[..n]);
                    if self.buf.len() > MAX_LINE && !self.buf.contains(&b'\n') {
                        self.buf.clear();
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::TimedOut => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }

    fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.port.write_all(format!("{line}\n").as_bytes())
    }
}
