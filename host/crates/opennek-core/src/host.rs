//! The host's parts wired together: the store behind the engine, and the radio feeding it and sending its downlinks

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use crate::engine::Engine;
use crate::radio::{Opener, Radio, RadioSettings};
use crate::store::Store;

pub struct Host {
    pub engine: Arc<Engine>,
    pub radio: Arc<Radio>,
    /// Set by `start`; a host made otherwise writes no log file
    pub log_file: OnceLock<PathBuf>,
}

impl Host {
    pub fn new(store: Store, opener: Box<dyn Opener>) -> Arc<Host> {
        let engine = Arc::new(Engine::new(store));
        let settings = {
            let engine = engine.clone();
            Box::new(move || {
                let s = engine.store().settings().unwrap_or_default();
                RadioSettings {
                    port: s.sigfox_port,
                    freq: s.sigfox_freq,
                    follow: s.sigfox_downlink,
                }
            })
        };
        let sink = {
            let engine = engine.clone();
            Box::new(move |e| engine.handle_radio(e))
        };
        let radio = Arc::new(Radio::new(settings, opener, sink));
        // The receiver also transmits the downlinks (firmware-lsm100a 0.3.0), while the sigfoxDownlink setting is on
        engine.set_transmitter(radio.clone());
        Arc::new(Host {
            radio,
            engine,
            log_file: OnceLock::new(),
        })
    }
}
