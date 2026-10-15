use codeterm_util::redact::text;
use serde_json::Value;

pub fn value(input: &Value) -> Value {
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
