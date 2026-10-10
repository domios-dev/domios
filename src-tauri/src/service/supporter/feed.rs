use serde_json::{json, Value};

pub(super) const MAX_ENTRIES: usize = 100;

pub(super) fn entry(value: &Value) -> Option<Value> {
    let kind = match value["kind"].as_str()? {
        "rerank" => "rerank",
        "blueprint" | "sentinel" => "blueprint",
        "heartbeat" => "heartbeat",
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
            "action_needed",
            "nothing_changed",
            "status_only",
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
            "decision",
            "route_claim",
            "notify",
            "send",
            "cron",
            "stop",
            "skip",
            "wake",
            "suppress",
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
            "provider_safe_step_authorized",
            "provider_safe_step_stale",
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
            "transition",
            "same_bucket",
            "low_confidence",
            "other",
            "budget_exhausted",
            "model_deadline",
            "model_failed",
            "adapter_disconnected",
            "invalid_answer",
            "route_failed",
            "hard_wake",
            "same_snapshot",
            "healthy_snapshot",
            "soft_cooldown",
            "safety_status",
            "observation_bounds",
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
    let mut entry = json!({
        "time": value["time"].as_i64()?, "team_id": identifier(&value["team_id"]),
        "kind": kind, "label": label, "confidence_band": band,
        "action": if action == "verdict" || action == "weak_hint" { "notify_only" } else { action },
        "reason": reason,
        "budget_reached": value["budget"]["reached"].as_bool(),
        "fallback": matches!(reason, "budget" | "credit_exhausted" | "deadline" | "unavailable" | "no_adapter"
            | "budget_exhausted" | "model_deadline" | "model_failed" | "adapter_disconnected" | "invalid_answer"),
    });
    if let Some(title) = value["tab_title"].as_str() {
        entry["tab_title"] = json!(super::narrative::scrub(title, super::narrative::MAX_NAME_CHARS));
    }
    entry["decision_point"] = json!(allowed(
        &value["decision_point"],
        &[
            "memory_rerank",
            "blueprint",
            "heartbeat",
            "tab_state",
            "launch_ready",
            "startup_deadline",
            "startup_escalation",
            "prompt_candidate",
            "worker_idle_without_report",
            "busy_without_output",
            "tab_exit",
            "user_delivery_readiness",
            "safe_response",
        ],
        kind,
    ));
    if value["observation_id"].as_u64().is_some() {
        entry["decided_by"] = json!(allowed(
            &value["decided_by"],
            &[
                "adapter",
                "heuristic",
                "fallback(no_adapter)",
                "fallback(timeout)",
                "fallback(error)",
                "fallback(budget)",
                "fallback(other)",
                "fallback(low_confidence)",
            ],
            "redacted"
        ));
        entry["observation_id"] = value["observation_id"].clone();
        if entry["decided_by"]
            .as_str()
            .is_some_and(|source| source.starts_with("fallback("))
        {
            entry["fallback"] = json!(true);
        }
        if entry["decided_by"] == "fallback(error)" {
            if let Some(category) = value["error_category"].as_str().filter(|category| {
                [
                    "transport",
                    "http",
                    "unavailable",
                    "unsupported",
                    "parse",
                    "invalid_observation",
                ]
                .contains(category)
            }) {
                entry["error_category"] = json!(category);
            }
        }
        crate::service::supporter_tab_state::project_observation_diagnostics(value, &mut entry);
    }
    let asked = if value["kind"] == "tab_state" {
        crate::service::supporter_tab_state::redact_decision_diagnostic(&value["asked"])
    } else {
        value["asked"].clone()
    };
    if let Some(asked) = super::narrative::project(&asked) {
        entry["asked"] = json!(asked);
    }
    Some(entry)
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

#[cfg(test)]
mod tests;
