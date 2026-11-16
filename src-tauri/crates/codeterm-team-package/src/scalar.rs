/// How a plain scalar resolves under the YAML 1.2 core schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlainKind {
    Null,
    Bool(bool),
    Int,
    Float,
    Str,
}

pub(crate) fn resolve_plain(text: &str) -> PlainKind {
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return PlainKind::Null,
        "true" | "True" | "TRUE" => return PlainKind::Bool(true),
        "false" | "False" | "FALSE" => return PlainKind::Bool(false),
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" | "-.inf" | "-.Inf" | "-.INF"
        | ".nan" | ".NaN" | ".NAN" => return PlainKind::Float,
        _ => {}
    }
    if is_int(text) {
        PlainKind::Int
    } else if is_float(text) {
        PlainKind::Float
    } else {
        PlainKind::Str
    }
}

fn is_int(text: &str) -> bool {
    if let Some(octal) = text.strip_prefix("0o") {
        return !octal.is_empty() && octal.bytes().all(|byte| (b'0'..=b'7').contains(&byte));
    }
    if let Some(hex) = text.strip_prefix("0x") {
        return !hex.is_empty() && hex.bytes().all(|byte| byte.is_ascii_hexdigit());
    }
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_float(text: &str) -> bool {
    let body = text.strip_prefix(['-', '+']).unwrap_or(text);
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(index) => (&body[..index], Some(&body[index + 1..])),
        None => (body, None),
    };
    let mantissa_ok = match mantissa.split_once('.') {
        Some((whole, fraction)) => {
            whole.bytes().all(|byte| byte.is_ascii_digit())
                && fraction.bytes().all(|byte| byte.is_ascii_digit())
                && !(whole.is_empty() && fraction.is_empty())
        }
        None => !mantissa.is_empty() && mantissa.bytes().all(|byte| byte.is_ascii_digit()),
    };
    let exponent_ok = exponent.is_none_or(|exponent| {
        let digits = exponent.strip_prefix(['-', '+']).unwrap_or(exponent);
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    });
    mantissa_ok && exponent_ok
}

/// Words YAML 1.1 readers turn into booleans; quoting them keeps packages portable.
const YAML11_BOOLEANS: &[&str] = &[
    "y", "Y", "yes", "Yes", "YES", "n", "N", "no", "No", "NO", "on", "On", "ON", "off", "Off",
    "OFF",
];

/// A character a literal or plain scalar cannot carry verbatim.
pub(crate) fn needs_escape(character: char) -> bool {
    matches!(character, '\0'..='\x08' | '\x0b'..='\x1f' | '\x7f' | '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{feff}')
}

/// True when `text` can be written plain and still read back as this exact string.
pub(crate) fn plain_safe(text: &str, flow: bool) -> bool {
    let Some(first) = text.chars().next() else {
        return false;
    };
    if text.starts_with([' ', '\t']) || text.ends_with([' ', '\t', ':']) {
        return false;
    }
    if "-?:,[]{}#&*!|>'\"%@`".contains(first) {
        return false;
    }
    if text
        .chars()
        .any(|character| character == '\n' || character == '\t' || needs_escape(character))
    {
        return false;
    }
    if text.contains(": ") || text.contains(" #") {
        return false;
    }
    if flow && text.contains([',', '[', ']', '{', '}']) {
        return false;
    }
    if text == "---" || text == "..." || text.starts_with("--- ") || text.starts_with("... ") {
        return false;
    }
    if YAML11_BOOLEANS.contains(&text) {
        return false;
    }
    resolve_plain(text) == PlainKind::Str
}

/// A double-quoted rendering that reads back as exactly `text`.
pub(crate) fn double_quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            character if needs_escape(character) => {
                let code = character as u32;
                if code <= 0xff {
                    out.push_str(&format!("\\x{code:02x}"));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests;
