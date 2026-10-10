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
    if token.get("token_mode").and_then(Value::as_str) == Some("none") {
        return "No token needed".into();
    }
    match token.get("present").and_then(Value::as_bool) {
        Some(true) => {
            let mode = match token.get("mode_ok").and_then(Value::as_bool) {
                Some(true) => "owner-only".to_string(),
                Some(false) => "permissions need attention".to_string(),
                None => "permissions n/a".to_string(),
            };
            format!("Token set ({mode})")
        }
        Some(false) => "No token".into(),
        None => "Token status unavailable".into(),
    }
}

fn state_text(status: &Value) -> String {
    match status.get("state").and_then(Value::as_str) {
        Some("active") => "Active".into(),
        Some("budget_reached") => "Budget reached".into(),
        Some("credit_exhausted") => "Credit exhausted".into(),
        Some("not_connected") => "Not connected".into(),
        Some("token_unusable") => "Token file needs attention".into(),
        Some("no_token") => "No token".into(),
        Some("adapter_unavailable") => "Adapter unavailable".into(),
        _ => "Unavailable".into(),
    }
}

pub(super) fn render_token_status(token: &Value) -> String {
    let adapter = token.get("adapter").and_then(Value::as_str).unwrap_or("?");
    if let Some(message) = token.get("message").and_then(Value::as_str) {
        return format!("{adapter}: {message}\n");
    }
    let mode = token.get("token_mode").and_then(Value::as_str);
    let mode = mode
        .map(|mode| format!(" (token mode: {mode})"))
        .unwrap_or_default();
    format!("{adapter}: {}{mode}\n", token_summary(token))
}

pub(super) fn render_token_set(token: &Value) -> String {
    let adapter = token.get("adapter").and_then(Value::as_str).unwrap_or("?");
    if let Some(message) = token.get("message").and_then(Value::as_str) {
        return format!("{adapter}: {message}\n");
    }
    format!("Token set for {adapter}\n")
}

pub(super) fn render_token_clear(reply: &Value) -> String {
    let adapter = reply.get("adapter").and_then(Value::as_str).unwrap_or("?");
    if let Some(message) = reply.get("message").and_then(Value::as_str) {
        return format!("{adapter}: {message}\n");
    }
    match reply.get("removed").and_then(Value::as_bool) {
        Some(true) => format!("Token removed for {adapter}\n"),
        _ => format!("No token was stored for {adapter}\n"),
    }
}

pub(super) fn request_cap(status: &Value) -> Option<u64> {
    status["budget"]["calls_per_hour"]
        .as_u64()
        .filter(|cap| *cap > 0)
}

pub(super) fn render_status(status: &Value) -> String {
    let adapter = status
        .get("adapter")
        .and_then(Value::as_str)
        .unwrap_or("none");
    let model = status.get("model").and_then(Value::as_str).unwrap_or("?");
    let token = status.get("token").unwrap_or(&Value::Null);
    let registered = status.get("adapter_registered").and_then(Value::as_bool) == Some(true);
    let mut out = format!(
        "{}\nAdapter: {} ({})\nModel: {}\nToken mode: {}\n{}\nDeadline: {} ms\n",
        state_text(status),
        adapter,
        if registered {
            "registered"
        } else {
            "not registered"
        },
        model,
        status
            .get("token_mode")
            .and_then(Value::as_str)
            .unwrap_or("required"),
        token_summary(token),
        status
            .get("deadline_ms")
            .and_then(Value::as_u64)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".into())
    );
    let cap = request_cap(status);
    let budget = cap.map_or_else(
        || "no cap".into(),
        |cap| {
            format!(
                "{} (cap {cap} / hour, free at {})",
                if status["budget"]["reached"] == true {
                    "reached"
                } else {
                    "available"
                },
                status["budget"]["free_at"]
            )
        },
    );
    out.push_str(&format!(
        "Calls: {} in the last hour / {} in the last day\nLast activity: {}\nBudget: {budget}\n",
        status["calls_last_hour"], status["calls_last_day"], status["last_activity"]
    ));
    for kind in ["rerank", "tab_state"] {
        if let Some(verdict) = status["last_verdict"].get(kind) {
            out.push_str(&format!(
                "Last {kind}: {} at {}\n",
                verdict["label"], verdict["time"]
            ));
        }
    }
    if let Some(spend) = status.get("spend") {
        for (key, label) in [("today", "Today"), ("month", "This month")] {
            out.push_str(&format!(
                "{label}: {} input / {} output tokens; estimated USD {}\n",
                spend[key]["input_tokens"],
                spend[key]["output_tokens"],
                spend[key]["estimated_usd"]
            ));
        }
        if spend["exhausted"] == true {
            out.push_str(&format!(
                "Credit exhausted; next request probe after {}\n",
                spend["next_probe_at"]
            ));
        }
    }
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
            let name = model
                .get("display_name")
                .and_then(Value::as_str)
                .unwrap_or(id);
            if name == id {
                id.to_string()
            } else {
                format!("{name} ({id})")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
