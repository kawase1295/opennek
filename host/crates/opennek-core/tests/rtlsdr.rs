use opennek_core::rtlsdr::{VirtualStreamer, is_rtlsdr, port_info};
use opennek_core::serial::PortInfo;

#[test]
fn the_status_line_gives_the_firmware_frequency_and_sample_rate_the_host_reads() {
    let s = VirtualStreamer::new(923_200_000);
    assert_eq!(s.status(), "I fw=rtl-sdr f=923200000 sr=2000 tx=0 hw=rtl-sdr");
}

#[test]
fn envelope_samples_go_out_as_r_lines_of_64_in_streamer_units() {
    let mut s = VirtualStreamer::new(923_200_000);
    // -72 dBFS reads -122 dBm: 244 = 0xF4 in -2 x dBm
    assert_eq!(s.samples(&[-72.0; 60]), Vec::<String>::new());
    let lines = s.samples(&[-72.0; 6]);
    assert_eq!(lines, [format!("R0000:{}", "F4".repeat(64))]);
    let lines = s.samples(&[0.0; 62]);
    assert_eq!(lines, [format!("R0001:{}{}", "F4".repeat(2), "64".repeat(62))]);
    // below what the protocol carries
    let lines = s.samples(&[-200.0; 64]);
    assert_eq!(lines, [format!("R0002:{}", "FF".repeat(64))]);
}

#[test]
fn the_sequence_number_wraps_like_the_firmwares() {
    let mut s = VirtualStreamer::new(923_200_000);
    let lines: Vec<String> = (0..0x10001).flat_map(|_| s.samples(&[-72.0; 64])).collect();
    assert!(lines[0xffff].starts_with("RFFFF:"));
    assert!(lines[0x10000].starts_with("R0000:"));
}

#[test]
fn commands_are_answered_as_the_streamer_does() {
    let mut s = VirtualStreamer::new(923_200_000);
    assert_eq!(s.command("?"), [s.status()]);
    assert_eq!(
        s.command("F922300000"),
        ["I fw=rtl-sdr f=922300000 sr=2000 tx=0 hw=rtl-sdr"]
    );
    assert_eq!(s.freq(), 922_300_000);
    // out of range: ignored, and the status still reported
    assert_eq!(s.command("F5"), [s.status()]);
    assert_eq!(s.freq(), 922_300_000);
    assert_eq!(s.command("C1"), [s.status()]);
    assert_eq!(s.command("E1"), [s.status()]);
    // it cannot transmit
    assert_eq!(s.command("T922183000,10,D1232EC88A6A34718AB5D1508837E4"), ["D off"]);
    assert_eq!(s.command("Z"), Vec::<String>::new());
    assert_eq!(s.command(""), Vec::<String>::new());
}

#[test]
fn rtl_sdrs_are_listed_as_ports_of_their_own() {
    assert_eq!(
        port_info(0, "RTL2838UHIDIR", "00000001"),
        PortInfo {
            path: "rtlsdr:0".into(),
            label: "RTL-SDR RTL2838UHIDIR 00000001".into(),
            kit: false,
        }
    );
    assert_eq!(port_info(1, "", "").label, "RTL-SDR");
    assert!(is_rtlsdr("rtlsdr:0"));
    assert!(!is_rtlsdr("/dev/ttyACM0"));
}
