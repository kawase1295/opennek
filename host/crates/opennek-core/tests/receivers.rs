use opennek_core::receivers::auto_port;
use opennek_core::rtlsdr::port_info;
use opennek_core::serial::PortInfo;
use opennek_core::store::ReceiverPort;

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

fn rtl(index: usize) -> PortInfo {
    port_info(index, "RTL2838UHIDIR", "00000001")
}

#[test]
fn the_kit_is_picked_only_when_it_is_the_one_kit_and_no_port_was_ever_set() {
    let unset = ReceiverPort::Unset;
    assert_eq!(auto_port(&[], &unset), None);
    assert_eq!(auto_port(&[kit_info("COM4")], &unset), Some("COM4".into()));
    assert_eq!(
        auto_port(&[info("COM1", "COM1"), kit_info("COM4"), info("COM5", "COM5")], &unset),
        Some("COM4".into())
    );
    assert_eq!(auto_port(&[info("COM1", "COM1")], &unset), None);
    // which one is the user's to say
    assert_eq!(auto_port(&[kit_info("COM4"), kit_info("COM5")], &unset), None);
    assert_eq!(auto_port(&[kit_info("COM4")], &ReceiverPort::Port("COM9".into())), None);
    assert_eq!(auto_port(&[kit_info("COM4")], &ReceiverPort::Off), None);
}

#[test]
fn an_rtl_sdr_is_picked_when_it_is_the_one_receiver_and_there_is_no_kit() {
    let unset = ReceiverPort::Unset;
    assert_eq!(
        auto_port(&[info("COM1", "COM1"), rtl(0)], &unset),
        Some("rtlsdr:0".into())
    );
    // the kit also transmits
    assert_eq!(auto_port(&[rtl(0), kit_info("COM4")], &unset), Some("COM4".into()));
    assert_eq!(auto_port(&[rtl(0), rtl(1)], &unset), None);
    assert_eq!(auto_port(&[rtl(0)], &ReceiverPort::Off), None);
}
