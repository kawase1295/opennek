//! What became of the downlink an uplink asked for, as the Sigfox Backend's SERVICE ACKNOWLEDGE callback tells it

use std::collections::HashMap;

use crate::store::{DownlinkStatus, Message};

/// How the host settled the downlink an uplink asked for. It is decided in this order: the data to send, the key to
/// sign it with, the transmitter, the timing, then the radio
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownlinkOutcome {
    /// Transmitted in the device's receive window
    Sent,
    /// No downlink data: downlink mode none, empty static data, or no chosen BIDIR callback answered with
    /// downlinkData (a 204, an HTTP error or a timeout included)
    NoData,
    /// The downlink data comes from a callback, but no enabled DATA BIDIR callback chosen for downlink applies to
    /// the device
    NoBidirCallback,
    /// The downlink data given is not 8 bytes of hex, or the device's static data no longer renders
    InvalidData,
    /// No key to sign the downlink with
    NotAuthenticated,
    /// No transmitter, or Send downlinks off
    ReceiveOnly,
    /// Frame 1 not received, or its frequency unknown: no time or frequency to aim at
    Frame1Unknown,
    WindowClosed,
    /// The transmitter did not send it
    Refused,
}

/// One row of the table: the Backend's infoCode, infoMessage and downlinkAck, and the status stored on the message
pub struct Report {
    pub info_code: u8,
    /// Put into JSON bodies as it is, so it has no quotes or backslashes
    pub info_message: &'static str,
    pub downlink_ack: bool,
    pub status: DownlinkStatus,
}

impl DownlinkOutcome {
    /// The one table of outcomes; docs/protocol.md has the same
    pub fn report(self) -> Report {
        use DownlinkOutcome::*;
        let (info_code, info_message, downlink_ack, status) = match self {
            Sent => (0, "Acked: the downlink was transmitted", true, DownlinkStatus::Sent),
            NoData => (
                1,
                "No answer: no downlink data was given",
                false,
                DownlinkStatus::Nodata,
            ),
            InvalidData => (
                2,
                "Invalid payload: the downlink data is not 8 bytes of hex",
                false,
                DownlinkStatus::Nodata,
            ),
            NotAuthenticated => (
                4,
                "Network error: not transmitted, the uplink is not authenticated",
                false,
                DownlinkStatus::Nodata,
            ),
            ReceiveOnly => (
                4,
                "Network error: not transmitted, OpenNEK is receiving only",
                false,
                DownlinkStatus::Unsupported,
            ),
            Frame1Unknown => (
                4,
                "Network error: not transmitted, the time or frequency of frame 1 is unknown",
                false,
                DownlinkStatus::Unsupported,
            ),
            WindowClosed => (
                4,
                "Network error: not transmitted, the device's receive window has closed",
                false,
                DownlinkStatus::Failed,
            ),
            Refused => (
                4,
                "Network error: not transmitted, the transmitter did not send it",
                false,
                DownlinkStatus::Failed,
            ),
            NoBidirCallback => (
                8,
                "Invalid configuration: the device takes its downlink from a callback, but no DATA BIDIR callback is chosen for downlink",
                false,
                DownlinkStatus::Nodata,
            ),
        };
        Report {
            info_code,
            info_message,
            downlink_ack,
            status,
        }
    }
}

/// The variables of an ACKNOWLEDGE callback, the Backend's only: OpenNEK has no downlink quota, so
/// {downlinkOverusage} is always false
pub fn acknowledge_vars(message: &Message, outcome: DownlinkOutcome) -> HashMap<String, String> {
    let r = outcome.report();
    [
        ("device", message.fields.device_id.clone()),
        ("time", message.fields.time.div_euclid(1000).to_string()),
        ("infoCode", r.info_code.to_string()),
        ("infoMessage", r.info_message.into()),
        ("downlinkAck", r.downlink_ack.to_string()),
        ("downlinkOverusage", "false".into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}
