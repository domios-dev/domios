use codeterm_types::agent::context::TurnContext;
use serde_json::{json, Value};

pub(super) const FLAG: &str = "--codeterm-statusline";
const FORWARD: &str = r#"payload=$(cat); if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then printf '%s' "$payload" | curl -s --max-time 2 -o /dev/null -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/context" -H 'Content-Type: application/json' -H "Authorization: Bearer $CODETERM_API_TOKEN" -H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE:-}" --data-binary @- || true; fi"#;

pub(super) fn parse(record: &Value) -> Option<TurnContext> {
    if record.get("type").and_then(Value::as_str) == Some("system")
        && record.get("subtype").and_then(Value::as_str) == Some("compact_boundary")
    {
        let used_tokens = record
            .pointer("/compactMetadata/postTokens")?
            .as_f64()
            .filter(|tokens| (0.0..=10_000_000.0).contains(tokens) && tokens.fract() == 0.0)?
            as u64;
        return Some(TurnContext {
            used_tokens,
            window_tokens: 0,
            cost_usd: None,
            source: "claude-jsonl-compaction".into(),
        });
    }
    let statusline = record.get("context_window");
    let usage = if let Some(statusline) = statusline {
        statusline.get("current_usage")?
    } else {
        if record.get("type")?.as_str()? != "assistant" {
            return None;
        }
        record.get("message")?.get("usage")?
    };
    let count = |value: &Value| value.as_u64().filter(|count| *count <= 10_000_000);
    let input = count(usage.get("input_tokens")?)?;
    let creation = count(
        usage
            .get("cache_creation_input_tokens")
            .unwrap_or(&Value::from(0)),
    )?;
    let read = count(
        usage
            .get("cache_read_input_tokens")
            .unwrap_or(&Value::from(0)),
    )?;
    let window = match statusline
        .and_then(|window| window.get("context_window_size"))
        .or_else(|| usage.get("context_window_size"))
    {
        Some(value) => count(value)?,
        None => 0,
    };
    let context = TurnContext {
        used_tokens: input.checked_add(creation)?.checked_add(read)?,
        window_tokens: window,
        cost_usd: record
            .pointer("/cost/total_cost_usd")
            .and_then(Value::as_f64)
            .filter(|cost| cost.is_finite() && (0.0..=1_000_000.0).contains(cost)),
        source: if statusline.is_some() {
            "claude-statusline"
        } else {
            "claude-jsonl"
        }
        .into(),
    };
    context.valid().then_some(context)
}

pub(super) fn settings(value: &Value) -> Option<String> {
    value.as_object()?;
    let line = value.get("statusLine");
    let command = match line {
        Some(line) if line.get("type")?.as_str()? == "command" => {
            let command = line.get("command")?.as_str()?;
            if command.trim().is_empty() || command.len() > 8192 {
                return None;
            }
            if let Some(padding) = line.get("padding") {
                if !padding.as_u64().is_some_and(|padding| padding <= 1000) {
                    return None;
                }
            }
            format!("{FORWARD}; printf '%s' \"$payload\" | ( {command}\n)")
        }
        Some(_) => return None,
        None => format!("{FORWARD}; true"),
    };
    let mut line_value = json!({"command":command,"type":"command"});
    if let Some(padding) = line.and_then(|line| line.get("padding")) {
        line_value["padding"] = padding.clone();
    }
    Some(json!({"statusLine":line_value}).to_string())
}

fn read_settings(path: &std::path::Path) -> Option<Value> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut text = String::new();
    file.take(65_537).read_to_string(&mut text).ok()?;
    if text.len() > 65_536 {
        return None;
    }
    let value: Value = serde_json::from_str(&text).ok()?;
    value.is_object().then_some(value)
}

fn compose_args(args: &[String]) -> Option<Vec<String>> {
    let enabled = args.iter().any(|arg| arg == FLAG);
    let mut rest = Vec::new();
    let mut overlay = serde_json::Map::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == FLAG || arg == super::model_capabilities::FLAG {
            continue;
        }
        let raw = if arg == "--settings" {
            Some(iter.next()?.as_str())
        } else {
            arg.strip_prefix("--settings=")
        };
        if let Some(raw) = raw {
            let value = serde_json::from_str::<Value>(raw)
                .ok()
                .filter(Value::is_object)
                .or_else(|| read_settings(std::path::Path::new(raw)))?;
            overlay.extend(value.as_object()?.clone());
        } else {
            rest.push(arg.clone());
        }
    }
    if enabled {
        let user = dirs::home_dir().and_then(|home| {
            let path = home.join(".claude/settings.json");
            if path.exists() {
                read_settings(&path)
            } else {
                Some(json!({}))
            }
        });
        if let Some(mut user) = user {
            user.as_object_mut()?.extend(overlay.clone());
            if let Some(composed) = settings(&user) {
                let composed: Value = serde_json::from_str(&composed).ok()?;
                overlay.extend(composed.as_object()?.clone());
            }
        }
    }
    overlay.insert("showThinkingSummaries".into(), Value::Bool(true));
    if let Some(window) = super::model_capabilities::compact_env(args) {
        let env = overlay.entry("env").or_insert_with(|| json!({}));
        env.as_object_mut()?.insert(
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW".into(),
            Value::String(window),
        );
    }
    rest.extend(["--settings".into(), Value::Object(overlay).to_string()]);
    Some(rest)
}

fn settings_file_args(mut composed: Vec<String>, path: String) -> Vec<String> {
    *composed.last_mut().expect("composed settings argument") = path;
    composed
}

pub(super) fn launch_args(args: &[String]) -> Option<Vec<String>> {
    let composed = compose_args(args)?;
    let path = codeterm_util::task_files::write(composed.last()?, Some(".ct-settings.json"))?;
    Some(settings_file_args(composed, path))
}

#[cfg(test)]
mod tests;
