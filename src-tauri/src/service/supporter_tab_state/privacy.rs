use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

const MAX_BYTES: usize = 8192;

pub(crate) fn text(input: &str) -> String {
    static SECRETS: OnceLock<Regex> = OnceLock::new();
    static URL: OnceLock<Regex> = OnceLock::new();
    static CREDENTIAL: OnceLock<Regex> = OnceLock::new();
    static CODE: OnceLock<Regex> = OnceLock::new();
    let credentials = CREDENTIAL.get_or_init(|| {
        Regex::new(
        r"(?i)\b(?:sk-[a-z0-9_-]+|gh[pousr]_[a-z0-9]+|eyJ[a-z0-9_-]+\.[a-z0-9_-]+\.[a-z0-9_-]+)"
    ).expect("credential pattern")
    });
    let urls = URL.get_or_init(|| Regex::new(r"https?://[^\s<>\x22]+").expect("URL pattern"));
    let labels = SECRETS.get_or_init(|| Regex::new(
        r#"(?im)(?:authorization|bearer|password|passwd|secret|token|api[_ -]?key|verification[_ -]?code|one[_ -]?time[_ -]?code|access[_ -]?code|device[_ -]?code|otp)["']?\s*[:= ]\s*[^\r\n]+"#
    ).expect("secret pattern"));
    let codes = CODE.get_or_init(|| {
        Regex::new(r"\b(?:[0-9]{6,8}|[A-Z0-9]{4}-[A-Z0-9]{4})\b").expect("code pattern")
    });
    let value = credentials.replace_all(input, "[redacted]");
    let value = codes.replace_all(&value, "[redacted]");
    let value = urls.replace_all(&value, |captures: &regex::Captures<'_>| {
        let url = &captures[0];
        let safe = url.split(['?', '#']).next().unwrap_or_default();
        let safe = if let Some((scheme, rest)) = safe.split_once("://") {
            format!("{scheme}://{}", rest.rsplit('@').next().unwrap_or_default())
        } else {
            safe.to_string()
        };
        if safe != url {
            format!("{safe}[redacted]")
        } else {
            safe
        }
    });
    labels.replace_all(&value, "[redacted]").into_owned()
}

pub(crate) fn screen(input: &str) -> String {
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
