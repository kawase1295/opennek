use opennek_core::platform::Os;
use opennek_core::serial::{ListedPort, PortInfo, UsbInfo, to_port_list};

fn adapter() -> Option<UsbInfo> {
    Some(UsbInfo {
        vid: 0x10c4,
        pid: 0xea60,
        manufacturer: Some("Silicon Labs".into()),
        serial_number: Some("0001".into()),
        product: Some("CP2102N USB to UART Bridge Controller".into()),
    })
}

// The XIAO ESP32S3's USB-Serial/JTAG
fn kit() -> Option<UsbInfo> {
    Some(UsbInfo {
        vid: 0x303a,
        pid: 0x1001,
        manufacturer: Some("Espressif".into()),
        serial_number: Some("D8:3B:DA:4A:1C:20".into()),
        product: Some("USB JTAG/serial debug unit".into()),
    })
}

fn port(path: &str, usb: Option<UsbInfo>) -> ListedPort {
    ListedPort { path: path.into(), usb }
}

fn info(path: &str, label: &str) -> PortInfo {
    PortInfo {
        path: path.into(),
        label: label.into(),
        kit: false,
    }
}

fn kit_info(path: &str) -> PortInfo {
    PortInfo {
        path: path.into(),
        label: "XIAO ESP32S3 kit".into(),
        kit: true,
    }
}

#[test]
fn linux_lists_usb_serial_devices_and_hides_the_built_in_tty_s_ports() {
    let ports = vec![
        port("/dev/ttyS0", None),
        port("/dev/ttyACM0", adapter()),
        port("/dev/ttyUSB0", None),
    ];
    assert_eq!(
        to_port_list(ports, Os::Linux),
        [
            info("/dev/ttyACM0", "Silicon Labs 0001"),
            info("/dev/ttyUSB0", "ttyUSB0")
        ]
    );
}

#[test]
fn windows_lists_every_com_port_with_its_device_name() {
    let ports = vec![port("COM4", adapter()), port("COM1", None), port("COM10", None)];
    assert_eq!(
        to_port_list(ports, Os::Windows),
        [
            info("COM1", "COM1"),
            info("COM4", "CP2102N USB to UART Bridge Controller"),
            info("COM10", "COM10")
        ]
    );
}

#[test]
fn macos_lists_usb_devices_once_on_their_callout_node() {
    let ports = vec![
        port("/dev/tty.Bluetooth-Incoming-Port", None),
        port("/dev/tty.usbserial-0001", adapter()),
        port("/dev/cu.usbserial-0001", adapter()),
    ];
    assert_eq!(
        to_port_list(ports, Os::MacOs),
        [info("/dev/cu.usbserial-0001", "Silicon Labs 0001")]
    );
}

#[test]
fn the_xiao_kit_is_marked_on_every_os() {
    for (os, path) in [
        (Os::Linux, "/dev/ttyACM0"),
        (Os::Windows, "COM4"),
        (Os::MacOs, "/dev/cu.usbmodem1101"),
    ] {
        assert_eq!(to_port_list(vec![port(path, kit())], os), [kit_info(path)], "{os:?}");
    }
    // macOS lists it twice, and it is still one kit
    let ports = vec![
        port("/dev/tty.usbmodem1101", kit()),
        port("/dev/cu.usbmodem1101", kit()),
    ];
    assert_eq!(to_port_list(ports, Os::MacOs), [kit_info("/dev/cu.usbmodem1101")]);
}
