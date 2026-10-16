//! RFC 8785 JSON Canonicalization Scheme over `serde_json::Value`.

use serde_json::{Map, Number, Value};
use std::fmt::Write;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JcsError {
    NonFinite,
}

pub fn encode(value: &Value) -> Result<String, JcsError> {
    let mut out = String::new();
    write_value(value, &mut out)?;
    Ok(out)
}

fn write_value(value: &Value, out: &mut String) -> Result<(), JcsError> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => write_number(number, out)?,
        Value::String(text) => write_string(text, out),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => write_object(map, out)?,
    }
    Ok(())
}

fn write_object(map: &Map<String, Value>, out: &mut String) -> Result<(), JcsError> {
    let mut entries: Vec<(&String, &Value)> = map.iter().collect();
    entries.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
    out.push('{');
    for (index, (key, value)) in entries.into_iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_string(key, out);
        out.push(':');
        write_value(value, out)?;
    }
    out.push('}');
    Ok(())
}

fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_number(number: &Number, out: &mut String) -> Result<(), JcsError> {
    if let Some(value) = number.as_u64() {
        if value <= MAX_SAFE_INTEGER {
            let _ = write!(out, "{value}");
            return Ok(());
        }
    } else if let Some(value) = number.as_i64() {
        if value.unsigned_abs() <= MAX_SAFE_INTEGER {
            let _ = write!(out, "{value}");
            return Ok(());
        }
    }
    let value = number.as_f64().ok_or(JcsError::NonFinite)?;
    out.push_str(&es_number(value)?);
    Ok(())
}

/// ECMAScript Number::toString for finite doubles (ECMA-262 7.1.12.1).
pub fn es_number(value: f64) -> Result<String, JcsError> {
    if !value.is_finite() {
        return Err(JcsError::NonFinite);
    }
    if value == 0.0 {
        return Ok("0".to_string());
    }
    // `{:e}` yields the shortest round-trip digits, which is exactly ES's k-digit significand.
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').ok_or(JcsError::NonFinite)?;
    let exponent: i32 = exponent.parse().map_err(|_| JcsError::NonFinite)?;
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exponent + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e < 0 { '-' } else { '+' };
        let significand = if k == 1 {
            digits
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        format!("{significand}e{sign}{}", e.unsigned_abs())
    };
    Ok(if value < 0.0 {
        format!("-{body}")
    } else {
        body
    })
}
