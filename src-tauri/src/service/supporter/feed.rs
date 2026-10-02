use serde_json::{json, Value};

pub(super) const MAX_ENTRIES: usize = 100;

pub(super) fn entry(value: &Value) -> Option<Value> {
    let kind = match value["kind"].as_str()? {
        "rerank" => "rerank",
        "tab_state" if value["action"] == "recovery" => "recovery",
        "tab_state" => "tab_state",
        _ => return None,
    };
    let label = allowed(
        &value["label"],
        &[
            "ranked",
            "relevance_scored",
            "ready_idle",
            "working_normally",
            "provider_update_prompt",
            "trust_or_permission_prompt",
            "login_or_auth_required",
            "model_unavailable_or_quota",
            "crashed_to_shell",
            "dead_pty",
            "error_loop",
            "other",
            "still_starting",
        ],
        if value["label"].is_null() && kind == "rerank" {
            "relevance_scored"
        } else {
            "redacted"
        },
    );
    let action = allowed(
        &value["action"],
        &[
            "spawn_ok",
            "strong_action",
            "rerank",
            "recovery",
            "weak_hint",
            "verdict",
        ],
        "notify_only",
    );
    let reason = allowed(
        &value["reason"],
        &[
            "model_answer",
            "all_candidates_scored",
            "launch_ready",
            "startup_deadline",
            "startup_escalation",
            "worker_idle_without_report",
            "busy_without_output",
            "tab_exit",
            "provider_declared_safe_choice",
            "deterministic_directory_trust",
            "verified_provider_absence",
            "confirmed_provider_exit_to_live_shell",
            "watchdog_dead_pty_event",
            "resumed",
            "fresh_launch",
            "budget",
            "credit_exhausted",
            "deadline",
            "unavailable",
            "no_adapter",
        ],
        "redacted",
    );
    let confidence = value["confidence"]
        .as_f64()
        .filter(|p| (0.0..=1.0).contains(p));
    let band = match confidence.map(crate::service::supporter_tab_state::confidence_band) {
        Some(crate::service::supporter_tab_state::ConfidenceBand::Strong) => "strong",
        Some(crate::service::supporter_tab_state::ConfidenceBand::Weak) => "weak",
        Some(_) => "low",
        None => "unknown",
    };
    Some(json!({
        "time": value["time"].as_i64()?, "team_id": identifier(&value["team_id"]),
        "kind": kind, "label": label, "confidence_band": band,
        "action": if action == "verdict" || action == "weak_hint" { "notify_only" } else { action },
        "reason": reason,
        "budget_reached": value["budget"]["reached"].as_bool(),
        "fallback": matches!(reason, "budget" | "credit_exhausted" | "deadline" | "unavailable" | "no_adapter"),
    }))
}

fn identifier(value: &Value) -> Option<&str> {
    value.as_str().filter(|id| {
        id.strip_prefix("team_").is_some_and(|suffix| {
            suffix.len() == 8 && suffix.bytes().all(|b| b.is_ascii_hexdigit())
        })
    })
}

fn allowed<'a>(value: &Value, choices: &[&'a str], fallback: &'a str) -> &'a str {
    choices
        .iter()
        .copied()
        .find(|choice| value.as_str() == Some(choice))
        .unwrap_or(fallback)
}
