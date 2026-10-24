//! Static downlink templates: 8 bytes of hex with the variables Sigfox emulators offer in between

pub const DOWNLINK_BYTES: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct DownlinkContext<'a> {
    /// The uplink's wall-clock ms
    pub time: i64,
    /// Hex station ID ('0000' from the receiver)
    pub station: &'a str,
    pub rssi: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variable {
    Time,
    Tapid,
    Rssi,
}

impl Variable {
    fn parse(name: &str) -> Option<Variable> {
        match name {
            "time" => Some(Variable::Time),
            "tapid" => Some(Variable::Tapid),
            "rssi" => Some(Variable::Rssi),
            _ => None,
        }
    }

    fn bytes(self) -> usize {
        match self {
            Variable::Time | Variable::Tapid => 4,
            Variable::Rssi => 2,
        }
    }

    fn render(self, c: &DownlinkContext) -> String {
        match self {
            Variable::Time => format!("{:08x}", c.time.div_euclid(1000) as u32),
            // a station that is not hex reads as 0
            Variable::Tapid => {
                let id = Some(c.station)
                    .filter(|s| (1..=8).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit()))
                    .and_then(|s| u32::from_str_radix(s, 16).ok())
                    .unwrap_or(0);
                format!("{id:08x}")
            }
            Variable::Rssi => format!("{:04x}", c.rssi.clamp(-0x8000, 0x7fff) as i16 as u16),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownlinkPart {
    Hex(String),
    Variable(Variable),
}

pub fn parse_downlink_template(template: &str) -> Result<Vec<DownlinkPart>, String> {
    let mut parts = vec![];
    let mut bytes = 0;
    let mut rest = template;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('{') {
            // an unclosed brace leaves text that is not hex
            let end = after.find('}').ok_or_else(|| format!("not hex: {template}"))?;
            let name = &after[..end];
            let v = Variable::parse(name)
                .ok_or_else(|| format!("unknown variable {{{name}}} (use {{time}}, {{tapid}} or {{rssi}})"))?;
            bytes += v.bytes();
            parts.push(DownlinkPart::Variable(v));
            rest = &after[end + 1..];
        } else {
            let end = rest.find('{').unwrap_or(rest.len());
            let hex = &rest[..end];
            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("not hex: {hex}"));
            }
            if !hex.len().is_multiple_of(2) {
                return Err(format!("hex needs an even number of digits: {hex}"));
            }
            bytes += hex.len() / 2;
            parts.push(DownlinkPart::Hex(hex.to_lowercase()));
            rest = &rest[end..];
        }
    }
    if bytes != DOWNLINK_BYTES {
        return Err(format!("must be {DOWNLINK_BYTES} bytes (got {bytes})"));
    }
    Ok(parts)
}

pub fn render_downlink(template: &str, ctx: &DownlinkContext) -> Result<String, String> {
    Ok(parse_downlink_template(template)?
        .iter()
        .map(|p| match p {
            DownlinkPart::Hex(h) => h.clone(),
            DownlinkPart::Variable(v) => v.render(ctx),
        })
        .collect())
}
