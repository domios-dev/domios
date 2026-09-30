use serde_json::Value;

const ENVELOPE_KEY: &str = "decision_rerank";

pub(super) fn percent(fraction: f64) -> String {
    format!("{:.0}%", fraction * 100.0)
}

pub(super) fn relevance_column(hit: &Value) -> Option<String> {
    let relevance = hit.get("relevance").and_then(Value::as_f64)?;
    Some(format!("rel {}", percent(relevance)))
}

/// One line explaining the envelope; `None` when the envelope is absent or needs no remark.
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
                "decision re-rank: nothing relevant was found above the confidence threshold{best}"
            ))
        }
        "fallback" => Some(format!(
            "decision re-rank: fallback to plain recall ({reason})"
        )),
        "skipped" => Some(format!("decision re-rank: skipped ({reason})")),
        _ => None,
    }
}

fn mode_label(token: &Value) -> String {
    match token.get("mode_ok").and_then(Value::as_bool) {
        Some(ok) => ok.to_string(),
        None => "n/a".into(),
    }
}

fn token_summary(token: &Value) -> String {
    if token.get("present").and_then(Value::as_bool) != Some(true) {
        return "no token stored".into();
    }
    let path = token.get("path").and_then(Value::as_str).unwrap_or("?");
    format!("token present | mode_ok={} | {path}", mode_label(token))
}

pub(super) fn render_token_status(token: &Value) -> String {
    let adapter = token.get("adapter").and_then(Value::as_str).unwrap_or("?");
    format!("{adapter}: {}\n", token_summary(token))
}

pub(super) fn render_token_set(token: &Value) -> String {
    let adapter = token.get("adapter").and_then(Value::as_str).unwrap_or("?");
    let path = token.get("path").and_then(Value::as_str).unwrap_or("?");
    format!(
        "token stored for {adapter}\npath: {path}\nmode_ok: {}\n",
        mode_label(token)
    )
}

pub(super) fn render_token_clear(reply: &Value) -> String {
    let adapter = reply.get("adapter").and_then(Value::as_str).unwrap_or("?");
    match reply.get("removed").and_then(Value::as_bool) {
        Some(true) => format!("token removed for {adapter}\n"),
        _ => format!("{adapter}: no token was stored\n"),
    }
}

pub(super) fn render_status(status: &Value) -> String {
    let config = status.get("config").unwrap_or(&Value::Null);
    let text = |key: &str| {
        config
            .pointer(&format!("/{}", key.replace('.', "/")))
            .map(|value| match value.as_str() {
                Some(text) => text.to_string(),
                None => value.to_string(),
            })
            .unwrap_or_else(|| "?".into())
    };
    let enabled = config.get("enabled").and_then(Value::as_bool) == Some(true);
    let registered = status.get("adapter_registered").and_then(Value::as_bool) == Some(true);
    let mut out = format!(
        "decision re-rank: {} | adapter {} ({})\n",
        if enabled { "enabled" } else { "disabled" },
        text("adapter"),
        if registered {
            "registered"
        } else {
            "not registered"
        },
    );
    out.push_str(&format!(
        "sources recall {} graph {} | max_concurrency {} | deadline_ms {} | low_confidence_threshold {} | max_body_chars {}\n",
        text("sources.recall"),
        text("sources.graph"),
        text("max_concurrency"),
        text("deadline_ms"),
        text("low_confidence_threshold"),
        text("max_body_chars"),
    ));
    let adapters = status
        .get("adapters")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if adapters.is_empty() {
        out.push_str("adapters: none registered\n");
        return out;
    }
    out.push_str("adapters:\n");
    for adapter in adapters {
        let id = adapter.get("id").and_then(Value::as_str).unwrap_or("?");
        let primitives = adapter
            .get("primitives")
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .filter(|(_, on)| on.as_bool() == Some(true))
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let token = token_summary(adapter.get("token").unwrap_or(&Value::Null));
        out.push_str(&format!("  {id} | primitives: {primitives} | {token}\n"));
    }
    out
}
