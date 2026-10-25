use serde_json::Value;

use crate::surface::runtime::{SurfaceCallOutcome, SurfaceEffect, SurfaceEvent};

fn boundary(c: char) -> bool {
    c.is_ascii_whitespace()
        || matches!(
            c,
            '?' | '#'
                | '&'
                | ';'
                | '"'
                | '\''
                | '<'
                | '>'
                | '`'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '\\'
        )
}

pub(super) fn text(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut start = Some(0);
    let mut copied = 0;
    let mut chars = raw.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if boundary(c) {
            start = Some(index + c.len_utf8());
        } else if c == '=' {
            let Some(key_start) = start.take() else {
                continue;
            };
            let name = url::form_urlencoded::parse(&raw.as_bytes()[key_start..index])
                .next()
                .map(|(name, _)| name.to_ascii_lowercase());
            if name.as_deref().is_some_and(|name| {
                matches!(
                    name,
                    "token" | "access_token" | "api_token" | "key" | "secret" | "password"
                )
            }) {
                let value_start = index + 1;
                while chars.peek().is_some_and(|(_, c)| !boundary(*c)) {
                    chars.next();
                }
                let end = chars.peek().map(|(index, _)| *index).unwrap_or(raw.len());
                output.push_str(&raw[copied..value_start]);
                output.push_str("REDACTED");
                copied = end;
            }
        }
    }
    output.push_str(&raw[copied..]);
    output
}

fn value(raw: &mut Value, sanitize: &impl Fn(&str) -> String) {
    match raw {
        Value::String(raw) => *raw = sanitize(raw),
        Value::Array(values) => values.iter_mut().for_each(|v| value(v, sanitize)),
        Value::Object(values) => values.values_mut().for_each(|v| value(v, sanitize)),
        _ => {}
    }
}

fn outcome_with(
    mut outcome: SurfaceCallOutcome,
    sanitize: &impl Fn(&str) -> String,
) -> SurfaceCallOutcome {
    value(&mut outcome.result, sanitize);
    for effect in &mut outcome.effects {
        let SurfaceEffect::DeliverAnnotation { markdown, .. } = effect;
        *markdown = sanitize(markdown);
    }
    outcome
}

fn event_with(mut event: SurfaceEvent, sanitize: &impl Fn(&str) -> String) -> SurfaceEvent {
    match &mut event {
        SurfaceEvent::State(state) => {
            for field in [
                &mut state.url,
                &mut state.title,
                &mut state.activity,
                &mut state.actor,
                &mut state.owner,
                &mut state.profile,
                &mut state.needs_human,
                &mut state.target_id,
            ] {
                *field = sanitize(field);
            }
            state
                .profiles
                .iter_mut()
                .for_each(|profile| *profile = sanitize(profile));
        }
        SurfaceEvent::Notice(notice) => {
            notice.text = sanitize(&notice.text);
            notice.detail = notice.detail.as_deref().map(sanitize);
        }
        SurfaceEvent::Frame(_) => {}
    }
    event
}

#[cfg(test)]
mod tests;

impl super::BrowserRuntime {
    pub(super) fn redact_text(&self, raw: &str) -> String {
        let mut result = raw.to_string();
        let secrets = self.secrets.lock();
        let mut ordered: Vec<_> = secrets.iter().collect();
        ordered.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        for secret in ordered {
            result = result.replace(secret.as_str(), "REDACTED");
            if let Ok(encoded) = serde_json::to_string(secret) {
                result = result.replace(&encoded[1..encoded.len() - 1], "REDACTED");
            }
        }
        text(&result)
    }
    pub(super) fn redact_outcome(&self, raw: SurfaceCallOutcome) -> SurfaceCallOutcome {
        outcome_with(raw, &|s| self.redact_text(s))
    }
    pub(super) fn redact_value(&self, raw: &mut Value) {
        value(raw, &|s| self.redact_text(s));
    }
    pub(super) fn redact_event(&self, raw: SurfaceEvent) -> SurfaceEvent {
        event_with(raw, &|s| self.redact_text(s))
    }
}

#[cfg(test)]
fn outcome(raw: SurfaceCallOutcome) -> SurfaceCallOutcome {
    outcome_with(raw, &text)
}
#[cfg(test)]
fn event(raw: SurfaceEvent) -> SurfaceEvent {
    event_with(raw, &text)
}
