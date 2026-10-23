//! The OpenNEK host: receives real Sigfox uplinks from a USB receiver, stores them and serves the web UI.

pub mod callbacks;
pub mod cli;
pub mod downlink;
pub mod engine;
pub mod host;
pub mod install;
pub mod payload;
pub mod platform;
pub mod radio;
pub mod serial;
pub mod server;
pub mod store;
