//! Sigfox uplink demodulation, frame decoding, authentication and downlink frames. No I/O: callers feed samples
//! and bytes and get values back.

pub mod auth;
pub mod demod;
pub mod downlink;
pub mod frame;
pub mod frequency;
