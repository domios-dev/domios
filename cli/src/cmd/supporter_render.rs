use serde_json::Value;

const ENVELOPE_KEY: &str = "supporter";

pub(super) fn percent(fraction: f64) -> String {
    format!("{:.0}%", fraction * 100.0)
}

pub(super) fn relevance_column(hit: &Value) -> Option<String> {
    let relevance = hit.get("relevance").and_then(Value::as_f64)?;
    Some(format!("rel {}", percent(relevance)))
}

pub(super) fn envelope_notice(response: &Value) -> Option<String> {
    let envelope = response.get(ENVELOPE_KEY)?;
    let status = envelope.get("status").and_then(Value::as_str)?;
    let reason = envelope
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("unspecified");
    match status {
        "applied" if envelope.get("low_confidence").and_then(Value::as_bool) == Some(true) => {
            let best = envelope
                .get("max_relevance")
                .and_then(Value::as_f64)
                .map(|max| format!(" (best relevance {})", percent(max)))
                .unwrap_or_default();
            Some(format!(
                "Supporter Agent: nothing relevant was found above the confidence threshold{best}"
            ))
        }
        "fallback" => Some(format!(
            "Supporter Agent: fallback to plain recall ({reason})"
        )),
        "skipped" => Some(format!("Supporter Agent: skipped ({reason})")),
        _ => None,
    }
}

fn token_summary(token: &Value) -> String {
    match token.get("present").and_then(Value::as_bool) {
        Some(true) => {
            let mode = match token.get("mode_ok").and_then(Value::as_bool) {
                Some(true) => "owner-only".to_string(),
                Some(false) => "permissions need attention".to_string(),
                None => "permissions n/a".to_string(),
            };
            format!("Token set ({mode})")
        }
        _ => "No token".into(),
    }
}

fn state_text(status: &Value) -> String {
    match status.get("state").and_then(Value::as_str) {
        Some("active") => "Active".into(),
        Some("token_only") => "Token set, all features off".into(),
        Some("token_unusable") => "Token file needs attention".into(),
        Some("no_token") => "No token".into(),
        Some("feature_without_token") => "Feature on, no token".into(),
        Some("adapter_unavailable") => "Adapter unavailable".into(),
        _ => "Unavailable".into(),
    }
}

pub(super) fn render_token_status(token: &Value) -> String {
    let adapter = token.get("adapter").and_then(Value::as_str).unwrap_or("?");
    format!("{adapter}: {}\n", token_summary(token))
}

pub(super) fn render_token_set(token: &Value) -> String {
    let adapter = token.get("adapter").and_then(Value::as_str).unwrap_or("?");
    format!("Token set for {adapter}\n")
}

pub(super) fn render_token_clear(reply: &Value) -> String {
    let adapter = reply.get("adapter").and_then(Value::as_str).unwrap_or("?");
    match reply.get("removed").and_then(Value::as_bool) {
        Some(true) => format!("Token removed for {adapter}\n"),
        _ => format!("No token was stored for {adapter}\n"),
    }
}

pub(super) fn render_status(status: &Value) -> String {
    let adapter = status.get("adapter").and_then(Value::as_str).unwrap_or("?");
    let model = status.get("model").and_then(Value::as_str).unwrap_or("?");
    let token = status.get("token").unwrap_or(&Value::Null);
    let registered = status.get("adapter_registered").and_then(Value::as_bool) == Some(true);
    let features = status.get("features").unwrap_or(&Value::Null);
    let mut out = format!(
        "{}\nAdapter: {} ({})\nModel: {}\n{}\nMemory re-rank: {}\nTab-state: {}\nDeadline: {} ms\n",
        state_text(status),
        adapter,
        if registered {
            "registered"
        } else {
            "not registered"
        },
        model,
        token_summary(token),
        on_off(features.get("memory_rerank")),
        on_off(features.get("tab_state")),
        status
            .get("deadline_ms")
            .and_then(Value::as_u64)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".into())
    );
    if token.get("mode_ok").and_then(Value::as_bool) == Some(false) {
        out.push_str("Token file permissions are not owner-only.\n");
    }
    let adapters = status
        .get("adapters")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if adapters.is_empty() {
        out.push_str("Registered adapters: none\n");
    } else {
        out.push_str("Registered adapters:\n");
        for adapter in adapters {
            let id = adapter.get("id").and_then(Value::as_str).unwrap_or("?");
            let primitives = adapter
                .get("primitives")
                .and_then(Value::as_object)
                .map(|map| {
                    map.iter()
                        .filter(|(_, value)| value.as_bool() == Some(true))
                        .map(|(name, _)| name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let models = model_names(adapter.get("models"));
            out.push_str(&format!(
                "  {id} | primitives: {primitives} | models: {models} | {}\n",
                token_summary(adapter.get("token").unwrap_or(&Value::Null))
            ));
        }
    }
    out
}

pub(super) fn render_models(body: &Value) -> String {
    let adapters = body
        .get("adapters")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if adapters.is_empty() {
        return "No registered Supporter Agent adapters.\n".into();
    }
    adapters
        .iter()
        .map(|adapter| {
            let id = adapter.get("id").and_then(Value::as_str).unwrap_or("?");
            format!("{id}: {}\n", model_names(adapter.get("models")))
        })
        .collect()
}

fn model_names(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|model| {
            let id = model.get("id").and_then(Value::as_str).unwrap_or("?");
            let name = model.get("display_name").and_then(Value::as_str).unwrap_or(id);
            if name == id {
                id.to_string()
            } else {
                format!("{name} ({id})")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn on_off(value: Option<&Value>) -> &'static str {
    if value.and_then(Value::as_bool) == Some(true) {
        "on"
    } else {
        "off"
    }
}
