//! Sigfox-compatible custom callbacks: variables ({device}, {data}, {customData#temp}, ...) in the URL, headers and
//! body, sent to the user's server

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::radio::now_ms;
use crate::store::{Auth, Callback, DownlinkStatus, Message, NewMessage};

const MAX_RESPONSE_LOG: usize = 2000;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallbackResult {
    pub status: Option<u16>,
    pub ok: bool,
    pub error: Option<String>,
    pub duration_ms: i64,
    pub response: Option<String>,
    /// 8 bytes of hex, only when the callback asks for downlink data and the response has it
    pub downlink: Option<String>,
}

// String(v) of the TypeScript host for decoded values
fn js_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(_) => "[object Object]".into(),
        Value::Array(a) => a
            .iter()
            .map(|x| if x.is_null() { String::new() } else { js_string(x) })
            .collect::<Vec<_>>()
            .join(","),
        other => other.to_string(),
    }
}

pub fn template_vars(m: &Message, device_name: &str) -> HashMap<String, String> {
    let f = &m.fields;
    let mut vars: HashMap<String, String> = [
        ("device", f.device_id.clone()),
        ("deviceName", device_name.into()),
        ("time", f.time.div_euclid(1000).to_string()),
        ("data", f.data.clone()),
        ("seqNumber", f.seq.to_string()),
        ("ack", f.ack.to_string()),
        ("oob", f.oob.to_string()),
        ("rssi", f.rssi.to_string()),
        ("snr", "0".into()),
        ("station", f.station.clone()),
        ("nbFrames", f.nb_frames.to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    if let Some(Value::Object(decoded)) = &f.decoded {
        for (k, v) in decoded {
            vars.insert(format!("customData#{k}"), js_string(v));
        }
    }
    vars
}

/// encodeURIComponent: everything but A-Z a-z 0-9 - _ . ! ~ * ' ( ) as UTF-8 percent escapes
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn is_var_name(name: &str) -> bool {
    let (word, custom) = match name.split_once('#') {
        Some((w, c)) => (w, Some(c)),
        None => (name, None),
    };
    !word.is_empty()
        && word.bytes().all(|b| b.is_ascii_alphabetic())
        && custom.is_none_or(|c| !c.is_empty() && c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
}

/// Replaces {name} and {name#field} with known variables; anything else stays as written
pub fn render_template(tpl: &str, vars: &HashMap<String, String>, encode: Option<fn(&str) -> String>) -> String {
    let mut out = String::with_capacity(tpl.len());
    let mut rest = tpl;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let replaced = after.find('}').and_then(|close| {
            let name = &after[..close];
            let value = vars.get(name).filter(|_| is_var_name(name))?;
            Some((encode.map_or_else(|| value.clone(), |e| e(value)), close))
        });
        match replaced {
            Some((value, close)) => {
                out.push_str(&value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn applies_to(cb: &Callback, device_id: &str) -> bool {
    let list: Vec<String> = cb
        .input
        .devices
        .split(',')
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .collect();
    list.is_empty() || list.iter().any(|d| d == device_id)
}

/// Downlink data from a response: Sigfox style ({"<ID>": {"downlinkData": ..}}, the ID in either case) or flat
pub fn extract_downlink(body: &str, device_id: &str) -> Option<String> {
    let json: Value = serde_json::from_str(body).ok()?;
    let obj = json.as_object()?;
    let data = [device_id.to_string(), device_id.to_lowercase()]
        .iter()
        .find_map(|id| obj.get(id).and_then(|d| d.get("downlinkData")).filter(|d| !d.is_null()))
        .or_else(|| obj.get("downlinkData"))?
        .as_str()?;
    (data.len() == 16 && data.bytes().all(|b| b.is_ascii_hexdigit())).then(|| data.to_lowercase())
}

/// A message to try a callback with when no real one applies
pub fn dummy_message(ack: bool) -> Message {
    Message {
        id: 0,
        fields: NewMessage {
            device_id: "F0000001".into(),
            time: now_ms(),
            seq: 0,
            data: "0102030405".into(),
            ack,
            oob: false,
            rssi: -60,
            nb_frames: 3,
            auth: Auth::Ok,
            station: "0000".into(),
            decoded: None,
            downlink: None,
            downlink_status: DownlinkStatus::None,
        },
    }
}

/// String.prototype.slice(0, n): at most n UTF-16 code units, without splitting a character
pub fn truncate_utf16(s: &str, n: usize) -> &str {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > n {
            return &s[..i];
        }
    }
    s
}

/// The URL as an http request target: what WHATWG URL parsing leaves unescaped but http::Uri rejects ({, |, ^...)
fn request_target(url: &url::Url) -> String {
    let mut out = String::with_capacity(url.as_str().len());
    for b in url.as_str().bytes() {
        if b.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=%".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

const MAX_REDIRECTS: usize = 20;
/// Headers that describe a body, dropped when a redirect turns the request into a GET
const BODY_HEADERS: [&str; 4] = [
    "content-type",
    "content-encoding",
    "content-language",
    "content-location",
];

struct Outgoing {
    method: String,
    url: url::Url,
    headers: Vec<(String, String)>,
    body: Option<String>,
}

impl Outgoing {
    fn send(&self, remaining: Duration) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
        // redirects are followed here, by the rules of fetch, which ureq does not apply to bodies
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(remaining))
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .into();
        let mut req = ureq::http::Request::builder()
            .method(self.method.as_str())
            .uri(request_target(&self.url));
        for (k, v) in &self.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        match &self.body {
            Some(body) => agent.run(req.body(body.clone())?),
            None => agent.run(req.body(())?),
        }
    }

    /// The request to make after a redirect to `location`, as fetch makes it
    fn redirected(mut self, status: u16, location: &str) -> Option<Outgoing> {
        let next = self.url.join(location).ok()?;
        let to_get = (matches!(status, 301 | 302) && self.method == "POST")
            || (status == 303 && !matches!(self.method.as_str(), "GET" | "HEAD"));
        if to_get {
            self.method = "GET".into();
            self.body = None;
            self.headers
                .retain(|(k, _)| !BODY_HEADERS.contains(&k.to_ascii_lowercase().as_str()));
        }
        if next.origin() != self.url.origin() {
            self.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
        }
        self.url = next;
        Some(self)
    }
}

fn is_timeout(e: &ureq::Error) -> bool {
    matches!(e, ureq::Error::Timeout(_)) || e.to_string().starts_with("timeout")
}

/// Sends one callback and waits for its answer (blocking)
pub fn run_callback(cb: &Callback, m: &Message, device_name: &str, timeout_ms: u32) -> CallbackResult {
    let started = Instant::now();
    let timeout = Duration::from_millis(timeout_ms.into());
    let elapsed = || started.elapsed().as_millis() as i64;
    let failed = |error: String| CallbackResult {
        status: None,
        ok: false,
        error: Some(error),
        duration_ms: elapsed(),
        response: None,
        downlink: None,
    };
    let fail_with = |e: ureq::Error| {
        failed(if is_timeout(&e) {
            format!("timeout ({timeout_ms} ms)")
        } else {
            e.to_string()
        })
    };
    let c = &cb.input;
    let vars = template_vars(m, device_name);
    let url = match url::Url::parse(&render_template(&c.url, &vars, Some(encode_uri_component))) {
        Ok(url) => url,
        Err(e) => return failed(format!("invalid URL: {e}")),
    };
    let mut headers: Vec<(String, String)> = c
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), render_template(v.as_str().unwrap_or_default(), &vars, None)))
        .collect();
    let has_body = c.method != "GET";
    if has_body && !headers.iter().any(|(k, _)| k == "Content-Type") {
        headers.push(("Content-Type".into(), c.content_type.clone()));
    }
    let body = has_body.then(|| render_template(&c.body, &vars, None));
    let mut out = Outgoing {
        method: c.method.clone(),
        url,
        headers,
        body,
    };
    for _ in 0..=MAX_REDIRECTS {
        let mut res = match out.send(timeout.saturating_sub(started.elapsed())) {
            Ok(res) => res,
            Err(e) => return fail_with(e),
        };
        let status = res.status().as_u16();
        let location = res
            .headers()
            .get("location")
            .and_then(|l| l.to_str().ok())
            .map(str::to_string);
        if let (301 | 302 | 303 | 307 | 308, Some(location)) = (status, &location) {
            match out.redirected(status, location) {
                Some(next) => {
                    out = next;
                    continue;
                }
                None => return failed(format!("invalid redirect location: {location}")),
            }
        }
        let bytes = match res.body_mut().with_config().limit(u64::MAX).read_to_vec() {
            Ok(b) => b,
            Err(e) => return fail_with(e),
        };
        // as Response.text(): lossy UTF-8 without a byte order mark
        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let ok = (200..300).contains(&status);
        return CallbackResult {
            status: Some(status),
            ok,
            error: (!ok).then(|| format!("HTTP {status}")),
            duration_ms: elapsed(),
            response: Some(truncate_utf16(text, MAX_RESPONSE_LOG).to_string()),
            downlink: (c.downlink && ok && status != 204)
                .then(|| extract_downlink(text, &m.fields.device_id))
                .flatten(),
        };
    }
    failed("too many redirects".into())
}
