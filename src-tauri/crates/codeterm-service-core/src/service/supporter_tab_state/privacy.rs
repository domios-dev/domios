use serde_json::Value;

const MAX_BYTES: usize = 8192;

pub use codeterm_util::redact::text;

pub fn screen(input: &str) -> String {
    let redacted = text(&super::screen_tail(input));
    let mut start = redacted.len().saturating_sub(MAX_BYTES);
    while !redacted.is_char_boundary(start) {
        start += 1;
    }
    redacted[start..].to_string()
}

pub(crate) fn complete_screen(input: &str) -> bool {
    input.trim_end().lines().count() <= super::SCREEN_LINES
        && screen(input) == super::screen_tail(input)
}

pub(crate) fn value(input: &Value) -> Value {
    match input {
        Value::String(s) => Value::String(text(s)),
        Value::Array(items) => Value::Array(items.iter().map(value).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, item)| {
                    let lower = key.to_ascii_lowercase();
                    let sensitive = [
                        "password",
                        "secret",
                        "token",
                        "credential",
                        "authorization",
                        "code",
                    ]
                    .iter()
                    .any(|part| lower.contains(part));
                    (
                        text(key),
                        if sensitive {
                            Value::String("[redacted]".into())
                        } else {
                            value(item)
                        },
                    )
                })
                .collect(),
        ),
        other => other.clone(),
    }
}
