//! Custom payload formats ("name:offset:type:args", space separated), the way Sigfox callbacks decode customData

use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Uint { bits: u32, little: bool },
    Int { bits: u32, little: bool },
    Float { bits: u32, little: bool },
    Bool { bit: u32 },
    Char { length: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    name: String,
    /// Byte offset; without one a field follows the previous one (bit cursor)
    offset: Option<usize>,
    kind: Kind,
}

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn parse_field(token: &str) -> Result<Field, String> {
    let mut parts = token.split(':');
    let (name, offset, kind) = (
        parts.next().unwrap_or(""),
        parts.next().unwrap_or(""),
        parts.next().unwrap_or(""),
    );
    let args: Vec<&str> = parts.collect();
    if !is_name(name) {
        return Err(format!("invalid field name in \"{token}\""));
    }
    let offset = match offset {
        "" => None,
        o => Some(
            o.parse::<usize>()
                .map_err(|_| format!("invalid offset in \"{token}\""))?,
        ),
    };
    let little = match args.get(1).copied() {
        None | Some("") | Some("big-endian") => false,
        Some("little-endian") => true,
        Some(_) => return Err(format!("invalid endianness in \"{token}\"")),
    };
    let n = args.first().and_then(|a| a.parse::<u32>().ok());
    let kind = match kind {
        "uint" | "int" => {
            let bits = n
                .filter(|b| (1..=32).contains(b))
                .ok_or_else(|| format!("bits must be 1..32 in \"{token}\""))?;
            if little && bits % 8 != 0 {
                return Err(format!("little-endian needs whole bytes in \"{token}\""));
            }
            if kind == "uint" {
                Kind::Uint { bits, little }
            } else {
                Kind::Int { bits, little }
            }
        }
        "float" => Kind::Float {
            bits: n
                .filter(|b| *b == 32 || *b == 64)
                .ok_or_else(|| format!("float must be 32 or 64 in \"{token}\""))?,
            little,
        },
        "bool" => {
            if offset.is_none() {
                return Err(format!("bool needs an explicit byte offset in \"{token}\""));
            }
            Kind::Bool {
                bit: n
                    .filter(|b| *b <= 7)
                    .ok_or_else(|| format!("bool bit must be 0..7 in \"{token}\""))?,
            }
        }
        "char" => Kind::Char {
            length: n
                .filter(|l| *l >= 1)
                .ok_or_else(|| format!("char length must be >= 1 in \"{token}\""))? as usize,
        },
        other => return Err(format!("unknown type \"{other}\" in \"{token}\"")),
    };
    Ok(Field {
        name: name.into(),
        offset,
        kind,
    })
}

pub fn parse_payload_config(config: &str) -> Result<Vec<Field>, String> {
    config.split_whitespace().map(parse_field).collect()
}

// Big-endian bit by bit, so fields need not start on a byte
fn read_bits(data: &[u8], start: usize, bits: u32) -> u64 {
    (start..start + bits as usize).fold(0, |v, b| v << 1 | u64::from(data[b >> 3] >> (7 - (b & 7)) & 1))
}

fn read_le(data: &[u8], start: usize, bits: u32) -> u64 {
    data[start / 8..start / 8 + bits as usize / 8]
        .iter()
        .rev()
        .fold(0, |v, &b| v << 8 | u64::from(b))
}

/// A float as JSON.stringify writes it, as far as it matters for payloads: whole numbers without a fraction (-0 as
/// 0), NaN and infinities as null. Beyond 2^53 and for tiny values serde's exponent notation remains
fn js_number(v: f64) -> Value {
    const SAFE: f64 = 9_007_199_254_740_992.0; // 2^53
    if v.fract() == 0.0 && v.abs() <= SAFE {
        Value::from(v as i64)
    } else {
        Value::from(v)
    }
}

/// The fields in the order of the config; a field that does not fit the payload fails the whole decode
pub fn decode_payload(config: &str, data: &[u8]) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    let mut cursor = 0;
    for f in parse_payload_config(config)? {
        let exceeds = || format!("field \"{}\" exceeds payload", f.name);
        let start = match f.offset {
            Some(o) => o.checked_mul(8).ok_or_else(exceeds)?,
            None => cursor,
        };
        let need = |bits: usize| match start.checked_add(bits) {
            Some(end) if end <= data.len() * 8 => Ok(()),
            _ => Err(exceeds()),
        };
        let aligned = || {
            if start % 8 != 0 {
                Err(format!("field \"{}\" is not byte aligned", f.name))
            } else {
                Ok(())
            }
        };
        let (value, width) = match f.kind {
            Kind::Bool { bit } => {
                need(8)?;
                // a bool reads its byte without moving the cursor
                out.insert(f.name, Value::Bool(data[start / 8] >> bit & 1 == 1));
                continue;
            }
            Kind::Char { length } => {
                aligned()?;
                need(length * 8)?;
                // TextDecoder drops a leading byte order mark
                let text = String::from_utf8_lossy(&data[start / 8..start / 8 + length]);
                (
                    text.strip_prefix('\u{feff}').unwrap_or(&text).to_string().into(),
                    length * 8,
                )
            }
            Kind::Float { bits, little } => {
                aligned()?;
                need(bits as usize)?;
                let bytes = &data[start / 8..start / 8 + bits as usize / 8];
                let v = match (bits, little) {
                    (32, false) => f64::from(f32::from_be_bytes(bytes.try_into().unwrap_or_default())),
                    (32, true) => f64::from(f32::from_le_bytes(bytes.try_into().unwrap_or_default())),
                    (_, false) => f64::from_be_bytes(bytes.try_into().unwrap_or_default()),
                    (_, true) => f64::from_le_bytes(bytes.try_into().unwrap_or_default()),
                };
                (js_number(v), bits as usize)
            }
            Kind::Uint { bits, little } | Kind::Int { bits, little } => {
                need(bits as usize)?;
                if little {
                    aligned()?;
                }
                let raw = if little {
                    read_le(data, start, bits)
                } else {
                    read_bits(data, start, bits)
                };
                let signed = matches!(f.kind, Kind::Int { .. }) && raw >> (bits - 1) & 1 == 1;
                let v = if signed {
                    raw as i64 - (1i64 << bits)
                } else {
                    raw as i64
                };
                (v.into(), bits as usize)
            }
        };
        out.insert(f.name, value);
        cursor = start + width;
    }
    Ok(out)
}
