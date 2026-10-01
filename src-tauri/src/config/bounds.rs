use super::schema::AppConfig;
use serde_json::Value;

#[derive(Clone, Copy)]
enum ValueKind {
    Integer,
    Number,
    Enum(&'static [&'static str]),
}

#[derive(Clone, Copy)]
struct ConfigBound {
    path: &'static str,
    kind: ValueKind,
    min: Option<f64>,
    max: Option<f64>,
}

const fn integer(path: &'static str, min: f64, max: f64) -> ConfigBound {
    ConfigBound {
        path,
        kind: ValueKind::Integer,
        min: Some(min),
        max: Some(max),
    }
}

const fn number(path: &'static str, min: f64, max: f64) -> ConfigBound {
    ConfigBound {
        path,
        kind: ValueKind::Number,
        min: Some(min),
        max: Some(max),
    }
}

const fn enumeration(path: &'static str, values: &'static [&'static str]) -> ConfigBound {
    ConfigBound {
        path,
        kind: ValueKind::Enum(values),
        min: None,
        max: None,
    }
}

const CONFIG_BOUNDS: &[ConfigBound] = &[
    integer("api.port", 1.0, 65535.0),
    integer("font.size", 8.0, 72.0),
    integer("terminal.scrollback", 100.0, 500000.0),
    enumeration("terminal.cursor_style", &["block", "underline", "bar"]),
    integer("notifications.toast_duration", 1.0, 60.0),
    enumeration(
        "notifications.agent_levels[].level",
        &["verbose", "action_needed", "failure"],
    ),
    integer("notifications.channels.*.per_scope_limit", 0.0, 100000.0),
    integer("agent.poll_interval_ms", 100.0, 60000.0),
    integer("agent.idle_timeout_s", 1.0, 3600.0),
    integer("general_agent.max_restart_attempts", 0.0, 100.0),
    integer("general_agent.restart_delay_s", 1.0, 86400.0),
    integer("orchestration.nudge_interval_secs", 60.0, 86400.0),
    integer("orchestration.refresh.handoff_timeout_secs", 1.0, 86400.0),
    integer("orchestration.refresh.settle_timeout_secs", 1.0, 86400.0),
    integer("orchestration.refresh.reorient_timeout_secs", 1.0, 86400.0),
    integer("orchestration.refresh.verify_timeout_secs", 1.0, 86400.0),
    integer("orchestration.digests.rules[].threshold", 0.0, 1000.0),
    integer(
        "orchestration.digests.rules[].stalled_threshold",
        0.0,
        1000.0,
    ),
    integer(
        "orchestration.digests.rules[].rate_limit_secs",
        1.0,
        604800.0,
    ),
    enumeration(
        "orchestration.digests.rules[].condition",
        &["in_review_gt", "in_progress_pressure"],
    ),
    integer("orchestration.reflect.every_gates", 0.0, 10000.0),
    integer("usage.freshness_secs", 1.0, 86400.0),
    integer("usage.standalone_cadence_secs", 0.0, 604800.0),
    integer("memory.workers_per_provider", 1.0, 16.0),
    integer("memory.worker_timeout_seconds", 1.0, 86400.0),
    integer("memory.dreaming_timeout_seconds", 1.0, 604800.0),
    number("memory.dedup_threshold", 0.0, 1.0),
    integer("memory.file_sync_interval_minutes", 1.0, 1440.0),
    integer("memory.l1_budget_tokens", 1.0, 100000.0),
    integer("supporter.memory_rerank.sources.recall", 1.0, 50.0),
    integer("supporter.memory_rerank.sources.graph", 0.0, 20.0),
    integer("supporter.memory_rerank.sources.docs", 0.0, 10.0),
    integer("supporter.memory_rerank.sources.sessions", 0.0, 5.0),
    integer("supporter.memory_rerank.max_concurrency", 1.0, 32.0),
    integer("supporter.deadline_ms", 100.0, 30000.0),
    integer("supporter.memory_rerank.candidates", 1.0, 50.0),
    number("supporter.memory_rerank.low_confidence_threshold", 0.0, 1.0),
    integer("supporter.memory_rerank.max_body_chars", 256.0, 100000.0),
    integer("memory.dream_hygiene.stale_archive_cap", 0.0, 500.0),
    integer("memory.dream_hygiene.merge_cap", 0.0, 100.0),
    integer("integrations.webhook_port", 1.0, 65535.0),
    integer("integrations.voice_transcribe.timeout_ms", 1.0, 600000.0),
    integer(
        "integrations.voice_transcribe.health_check_ttl_secs",
        1.0,
        86400.0,
    ),
    integer("integrations.text_hooks.timeout_ms", 1.0, 600000.0),
    integer(
        "integrations.text_hooks.health_check_ttl_secs",
        1.0,
        86400.0,
    ),
    enumeration(
        "integrations.text_hooks.fallback_on_error",
        &["forward", "drop"],
    ),
    integer("mesh.broadcast_duration_secs", 1.0, 86400.0),
    integer("runaway.test_binary_max_secs", 1.0, 86400.0),
];

const MAX_CONFIG_STRING_BYTES: usize = 64 * 1024;
const MAX_CONFIG_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_CONFIG_COLLECTION_ITEMS: usize = 4096;
const MAX_CONFIG_KEY_BYTES: usize = 256;

pub fn validate_app_config(config: &AppConfig) -> Result<(), String> {
    let value =
        serde_json::to_value(config).map_err(|error| format!("cannot validate config: {error}"))?;
    validate_config_value(&value)
}

pub fn validate_config_value(value: &Value) -> Result<(), String> {
    let mut total_bytes = 0;
    validate_input_size(value, "$", &mut total_bytes)?;
    for bound in CONFIG_BOUNDS {
        for candidate in values_at_path(value, bound.path) {
            validate_bound(bound, candidate)?;
        }
    }
    Ok(())
}

fn validate_input_size(value: &Value, path: &str, total_bytes: &mut usize) -> Result<(), String> {
    match value {
        Value::String(text) => {
            if text.len() > MAX_CONFIG_STRING_BYTES {
                return Err(format!(
                    "config value at {path} exceeds {MAX_CONFIG_STRING_BYTES} UTF-8 bytes"
                ));
            }
            validate_control_characters(path, text)?;
            add_config_bytes(total_bytes, text.len())
        }
        Value::Array(items) => {
            if items.len() > MAX_CONFIG_COLLECTION_ITEMS {
                return Err(format!(
                    "config collection at {path} exceeds {MAX_CONFIG_COLLECTION_ITEMS} items"
                ));
            }
            for (index, item) in items.iter().enumerate() {
                validate_input_size(item, &format!("{path}[{index}]"), total_bytes)?;
            }
            Ok(())
        }
        Value::Object(fields) => {
            if fields.len() > MAX_CONFIG_COLLECTION_ITEMS {
                return Err(format!(
                    "config object at {path} exceeds {MAX_CONFIG_COLLECTION_ITEMS} keys"
                ));
            }
            for (key, child) in fields {
                if key.len() > MAX_CONFIG_KEY_BYTES {
                    return Err(format!(
                        "config key at {path} exceeds {MAX_CONFIG_KEY_BYTES} UTF-8 bytes"
                    ));
                }
                validate_control_characters(path, key)?;
                add_config_bytes(total_bytes, key.len())?;
                validate_input_size(child, &format!("{path}.{key}"), total_bytes)?;
            }
            Ok(())
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
    }
}

fn validate_control_characters(path: &str, value: &str) -> Result<(), String> {
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        Err(format!(
            "config value at {path} contains a control character"
        ))
    } else {
        Ok(())
    }
}

fn add_config_bytes(total_bytes: &mut usize, bytes: usize) -> Result<(), String> {
    *total_bytes = total_bytes.saturating_add(bytes);
    if *total_bytes > MAX_CONFIG_TOTAL_BYTES {
        return Err(format!(
            "config values exceed {MAX_CONFIG_TOTAL_BYTES} total UTF-8 bytes"
        ));
    }
    Ok(())
}

pub fn validate_config_text(text: &str) -> Result<(), String> {
    let config: AppConfig = toml::from_str(text).map_err(|error| error.to_string())?;
    validate_app_config(&config)
}

fn validate_bound(bound: &ConfigBound, value: &Value) -> Result<(), String> {
    match bound.kind {
        ValueKind::Integer => {
            let numeric = value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
                .map(|number| number as f64)
                .ok_or_else(|| format!("config key {} must be an integer", bound.path))?;
            validate_range(bound, numeric)
        }
        ValueKind::Number => {
            let numeric = value
                .as_f64()
                .ok_or_else(|| format!("config key {} must be a number", bound.path))?;
            validate_range(bound, numeric)
        }
        ValueKind::Enum(allowed) => {
            let candidate = value
                .as_str()
                .ok_or_else(|| format!("config key {} must be a string", bound.path))?;
            if allowed.contains(&candidate) {
                Ok(())
            } else {
                Err(format!(
                    "config key {} must be one of: {}",
                    bound.path,
                    allowed.join(", ")
                ))
            }
        }
    }
}

fn validate_range(bound: &ConfigBound, value: f64) -> Result<(), String> {
    let (Some(min), Some(max)) = (bound.min, bound.max) else {
        return Ok(());
    };
    if value < min || value > max {
        return Err(format!(
            "config key {} must be in range {}..={}",
            bound.path,
            format_number(min),
            format_number(max)
        ));
    }
    Ok(())
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn values_at_path<'a>(value: &'a Value, path: &str) -> Vec<&'a Value> {
    values_at_segments(value, &path.split('.').collect::<Vec<_>>())
}

fn values_at_segments<'a>(value: &'a Value, segments: &[&str]) -> Vec<&'a Value> {
    let Some((segment, remaining)) = segments.split_first() else {
        return vec![value];
    };
    if *segment == "*" {
        return value
            .as_object()
            .into_iter()
            .flat_map(|object| object.values())
            .flat_map(|child| values_at_segments(child, remaining))
            .collect();
    }
    if let Some(key) = segment.strip_suffix("[]") {
        return value
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|child| values_at_segments(child, remaining))
            .collect();
    }
    value
        .get(*segment)
        .map(|child| values_at_segments(child, remaining))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "bounds_tests.rs"]
mod tests;
