use opennek_core::console::{ConsoleBuffer, ConsoleLine, radio_line};
use opennek_core::radio::{RadioState, RadioStatus};

fn line(n: i64) -> ConsoleLine {
    ConsoleLine {
        line: format!("line {n}"),
        time: n,
    }
}

#[test]
fn the_buffer_keeps_the_lines_oldest_first() {
    let mut buffer = ConsoleBuffer::new(3);
    buffer.push(line(1));
    buffer.push(line(2));
    assert_eq!(buffer.lines(), [line(1), line(2)]);
}

#[test]
fn a_full_buffer_drops_the_oldest_line() {
    let mut buffer = ConsoleBuffer::new(3);
    for n in 1..=4 {
        buffer.push(line(n));
    }
    assert_eq!(buffer.lines(), [line(2), line(3), line(4)]);
}

#[test]
fn the_radio_line_has_what_is_known_of_the_receiver() {
    let off = RadioStatus::default();
    assert_eq!(radio_line(&off), "sigfox: off");
    let connected = RadioStatus {
        state: RadioState::Connected,
        port: Some("/dev/ttyACM0".into()),
        fw: Some("0.3.0".into()),
        freq: Some(923_200_000),
        error: Some("busy".into()),
        frames: 12,
        ..Default::default()
    };
    assert_eq!(
        radio_line(&connected),
        "sigfox: connected /dev/ttyACM0 fw=0.3.0 923.200 MHz (busy)"
    );
}
