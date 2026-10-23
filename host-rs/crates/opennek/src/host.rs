//! The host's parts wired together: the store behind the engine, and the radio feeding it

use std::sync::Arc;

use crate::engine::Engine;
use crate::radio::{Opener, Radio, RadioSettings};
use crate::store::Store;

pub struct Host {
    pub engine: Arc<Engine>,
    pub radio: Radio,
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
                }
            })
        };
        let sink = {
            let engine = engine.clone();
            Box::new(move |e| engine.handle_radio(e))
        };
        Arc::new(Host {
            radio: Radio::new(settings, opener, sink),
            engine,
        })
    }
}
