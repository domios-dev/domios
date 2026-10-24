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
            let name = url::form_urlencoded::parse(raw[key_start..index].as_bytes())
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

fn value(raw: &mut Value) {
    match raw {
        Value::String(raw) => *raw = text(raw),
        Value::Array(values) => values.iter_mut().for_each(value),
        Value::Object(values) => values.values_mut().for_each(value),
        _ => {}
    }
}

pub(super) fn outcome(mut outcome: SurfaceCallOutcome) -> SurfaceCallOutcome {
    value(&mut outcome.result);
    for effect in &mut outcome.effects {
        let SurfaceEffect::DeliverAnnotation { markdown, .. } = effect;
        *markdown = text(markdown);
    }
    outcome
}

pub(super) fn event(mut event: SurfaceEvent) -> SurfaceEvent {
    match &mut event {
        SurfaceEvent::State(state) => {
            for field in [
                &mut state.url,
                &mut state.title,
                &mut state.activity,
                &mut state.actor,
                &mut state.owner,
                &mut state.profile,
            ] {
                *field = text(field);
            }
            state
                .profiles
                .iter_mut()
                .for_each(|profile| *profile = text(profile));
        }
        SurfaceEvent::Notice(notice) => {
            notice.text = text(&notice.text);
            notice.detail = notice.detail.as_deref().map(text);
        }
        SurfaceEvent::Frame(_) => {}
    }
    event
}

#[cfg(test)]
mod tests;
