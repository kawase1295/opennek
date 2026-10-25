use opennek_core::platform::Os;
use opennek_core::serial::{ListedPort, PortInfo, UsbInfo, to_port_list};

fn espressif() -> Option<UsbInfo> {
    Some(UsbInfo {
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
    }
}

#[test]
fn linux_lists_usb_serial_devices_and_hides_the_built_in_tty_s_ports() {
    let ports = vec![
        port("/dev/ttyS0", None),
        port("/dev/ttyACM0", espressif()),
        port("/dev/ttyUSB0", None),
    ];
    assert_eq!(
        to_port_list(ports, Os::Linux),
        [
            info("/dev/ttyACM0", "Espressif D8:3B:DA:4A:1C:20"),
            info("/dev/ttyUSB0", "ttyUSB0")
        ]
    );
}

#[test]
fn windows_lists_every_com_port_with_its_device_name() {
    let named = |product: &str| {
        Some(UsbInfo {
            product: Some(product.into()),
            ..espressif().unwrap()
        })
    };
    let ports = vec![
        port("COM4", named("USB Serial Device (COM4)")),
        port("COM1", None),
        port("COM10", None),
    ];
    assert_eq!(
        to_port_list(ports, Os::Windows),
        [
            info("COM1", "COM1"),
            info("COM4", "USB Serial Device (COM4)"),
            info("COM10", "COM10")
        ]
    );
}

#[test]
fn macos_lists_usb_devices_once_on_their_callout_node() {
    let ports = vec![
        port("/dev/tty.Bluetooth-Incoming-Port", None),
        port("/dev/tty.usbmodem1101", espressif()),
        port("/dev/cu.usbmodem1101", espressif()),
    ];
    assert_eq!(
        to_port_list(ports, Os::MacOs),
        [info("/dev/cu.usbmodem1101", "Espressif D8:3B:DA:4A:1C:20")]
    );
}
