//! The API's types for the web UI, kept in `host/ui/src/lib/api.gen.ts`. The UI is type checked against that
//! file, so a change to these types shows up there; regenerate it with
//! `UPDATE_API_TYPES=1 cargo test -p opennek-core api_types`

use std::path::Path;

use ts_rs::{Config, TS};

use crate::callbacks::CallbackResult;
use crate::console::ConsoleLine;
use crate::engine::Event;
use crate::radio::{RadioState, RadioStatus};
use crate::serial::PortInfo;
use crate::server::{CallbackSummary, Status, TestResult};
use crate::store::{
    Auth, Callback, CallbackInput, CallbackLog, CallbackSubtype, CallbackType, Channel, Device, DownlinkStatus,
    HttpMethod, Language, Message, ReceiverPort, Settings, Theme,
};

const HEADER: &str = "// Generated from the host's Rust types by `UPDATE_API_TYPES=1 cargo test -p opennek-core api_types`.\n\
                      // Do not edit: change the Rust types and regenerate.\n";

fn declarations() -> String {
    // JSON numbers; no value of the API comes near 2^53
    let cfg = Config::new().with_large_int("number");
    let decls = [
        Settings::decl(&cfg),
        ReceiverPort::decl(&cfg),
        Language::decl(&cfg),
        Theme::decl(&cfg),
        Device::decl(&cfg),
        CallbackInput::decl(&cfg),
        CallbackType::decl(&cfg),
        CallbackSubtype::decl(&cfg),
        Channel::decl(&cfg),
        HttpMethod::decl(&cfg),
        Callback::decl(&cfg),
        CallbackSummary::decl(&cfg),
        Auth::decl(&cfg),
        DownlinkStatus::decl(&cfg),
        Message::decl(&cfg),
        CallbackLog::decl(&cfg),
        CallbackResult::decl(&cfg),
        RadioState::decl(&cfg),
        RadioStatus::decl(&cfg),
        ConsoleLine::decl(&cfg),
        PortInfo::decl(&cfg),
        Status::decl(&cfg),
        TestResult::decl(&cfg),
        Event::decl(&cfg),
    ];
    let body: Vec<String> = decls.iter().map(|d| format!("export {d}\n")).collect();
    format!("{HEADER}\n{}", body.join("\n"))
}

#[test]
fn the_ui_has_the_api_types() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/lib/api.gen.ts");
    let want = declarations();
    if std::env::var_os("UPDATE_API_TYPES").is_some() {
        std::fs::write(&path, &want).unwrap();
        return;
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "{} is not up to date with the API's types; run `UPDATE_API_TYPES=1 cargo test -p opennek-core api_types`",
        path.display()
    );
}
