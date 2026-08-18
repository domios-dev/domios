use crate::args::agent::{AgentAction, SpawnRole};
use crate::client::SocketClient;

pub(super) fn validate_spawn_context(
    role: Option<&SpawnRole>,
    has_workspace: bool,
    has_parent: bool,
    has_plan: bool,
    has_bound_task: bool,
    has_plan_task: bool,
) -> Result<(), &'static str> {
    if matches!(role, Some(SpawnRole::Orchestrator)) && !has_workspace && !has_parent {
        return Err("--workspace is required when spawning --role Orchestrator");
    }
    if matches!(role, Some(SpawnRole::Planner)) && !has_plan && !has_parent {
        return Err("--plan <id> is required when spawning --role Planner (or pass --parent)");
    }
    if matches!(role, Some(SpawnRole::Worker)) && has_plan && !has_bound_task && !has_plan_task {
        return Err(
            "--role Worker with --plan requires --task-id or --plan-task; omit --role/--plan for a roleless solo agent",
        );
    }
    Ok(())
}

/// Resolve the `--team` attach fields for `agent spawn`. Team membership is
/// authorized against the caller's own tab, so the caller id is mandatory; the
/// mesh route refuses `--team` before it ever reaches here.
pub(super) fn team_spawn_fields(
    team_id: &str,
    caller_tab_id: Option<&str>,
) -> Result<(String, String), String> {
    let team_id = team_id.trim();
    if team_id.is_empty() {
        return Err("--team requires a Team id".into());
    }
    let caller_tab_id = caller_tab_id
        .map(str::trim)
        .filter(|caller| !caller.is_empty())
        .ok_or_else(|| "--team requires CODETERM_TAB_ID from a live Team member".to_string())?;
    Ok((team_id.to_string(), caller_tab_id.to_string()))
}

/// The `agent spawn` verb: validate the flag combination, resolve file-backed
/// inputs, and build the `agent.spawn` request.
pub(super) fn handle(client: &mut SocketClient, json: bool, action: AgentAction) {
    let AgentAction::Spawn {
        agent,
        title,
        description,
        cwd,
        role,
        parent,
        plan,
        report_to,
        can_spawn,
        task,
        task_file,
        args,
        workspace,
        model,
        reasoning_effort,
        system_prompt,
        preset,
        charter,
        charter_file,
        interval,
        nudge_cooldown,
        max_actions,
        bound_task,
        plan_task,
        team,
        execution_intent,
        provider_reserve_pct,
        non_user_facing,
    } = action
    else {
        unreachable!("agent_spawn::handle received a non-spawn action")
    };
    if let Err(error) = validate_spawn_context(
        role.as_ref(),
        workspace.is_some(),
        parent.is_some(),
        plan.is_some(),
        bound_task.is_some(),
        plan_task.is_some(),
    ) {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
    if task_file.is_none() {
        if let Some(t) = task.as_deref() {
            crate::cmd::send_text::nudge_spawn_task(json, t);
        }
    }
    let task = match crate::cmd::agent::resolve_spawn_task(
        task,
        task_file,
        cwd.as_deref(),
        &mut std::io::stdin(),
    ) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };
    let charter = match crate::cmd::agent::resolve_spawn_charter(
        charter,
        charter_file,
        &mut std::io::stdin(),
    ) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };
    let interval = match interval
        .as_deref()
        .map(|v| crate::cmd::agent::parse_duration_secs(v, "--interval"))
        .transpose()
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };
    let nudge_cooldown = match nudge_cooldown
        .as_deref()
        .map(|v| crate::cmd::agent::parse_duration_secs(v, "--nudge-cooldown"))
        .transpose()
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };
    for hint in crate::cmd::hints::worker_spawn_hints(
        matches!(role, Some(crate::args::agent::SpawnRole::Worker)),
        parent.is_some() || report_to.is_some(),
        task.is_some(),
    ) {
        eprintln!("{hint}");
    }
    let mut body = serde_json::json!({
        "agent": agent,
    });
    if let Some(t) = crate::cmd::agent::join_cli_words(title) {
        body["title"] = serde_json::json!(t);
    }
    if let Some(d) = crate::cmd::agent::join_cli_words(description) {
        match crate::cmd::agent::normalize_tab_description(&d) {
            Ok(Some(d)) => body["description"] = serde_json::json!(d),
            Ok(None) => body["description"] = serde_json::json!(""),
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }
    if let Some(c) = cwd {
        body["cwd"] = serde_json::json!(c);
    }
    if let Some(r) = role {
        body["role"] = serde_json::json!(r.as_str());
        if let Some(profile) = r.role_profile() {
            body["role_profile"] = serde_json::json!(profile);
        }
    }
    if let Some(p) = parent {
        body["parent_tab_id"] = serde_json::json!(p);
    }
    if let Some(pl) = plan {
        body["plan_id"] = serde_json::json!(pl);
    }
    if let Some(rt) = report_to {
        body["report_to"] = serde_json::json!(rt);
    }
    if can_spawn {
        body["can_spawn"] = serde_json::json!(true);
    }
    if let Some(t) = task {
        body["task"] = serde_json::json!(t);
    }
    if let Some(a) = args {
        body["args"] = serde_json::json!(a);
    }
    if let Some(w) = workspace {
        body["workspace_id"] = serde_json::json!(w);
    }
    if let Some(m) = model {
        body["model"] = serde_json::json!(m);
    }
    if let Some(effort) = reasoning_effort {
        body["reasoning_effort"] = serde_json::json!(effort);
    }
    if let Some(system_prompt) = system_prompt {
        body["system_prompt"] = serde_json::json!(system_prompt);
    }
    if let Some(preset) = preset {
        body["preset"] = serde_json::json!(preset);
    }
    if let Some(charter) = charter {
        body["charter"] = serde_json::json!(charter);
    }
    if let Some(interval) = interval {
        body["tick_interval_secs"] = serde_json::json!(interval);
    }
    if let Some(nudge_cooldown) = nudge_cooldown {
        body["nudge_cooldown_secs"] = serde_json::json!(nudge_cooldown);
    }
    if let Some(max_actions) = max_actions {
        body["max_actions_per_tick"] = serde_json::json!(max_actions);
    }
    if let Some(tid) = bound_task {
        body["task_id"] = serde_json::json!(tid);
    }
    if let Some(pt) = plan_task {
        body["plan_task"] = serde_json::json!(pt);
    }
    if let Some(team_id) = team.as_deref() {
        match team_spawn_fields(team_id, std::env::var("CODETERM_TAB_ID").ok().as_deref()) {
            Ok((team_id, creator_tab_id)) => {
                body["team_id"] = serde_json::json!(team_id);
                body["creator_tab_id"] = serde_json::json!(creator_tab_id);
            }
            Err(error) => {
                eprintln!("Error: {error}");
                std::process::exit(1);
            }
        }
    }
    if let Some(intent) = execution_intent {
        body["execution_intent"] = serde_json::json!(intent);
        body["provider_reserve_pct"] = serde_json::json!(provider_reserve_pct);
    }
    if non_user_facing {
        body["audience"] = serde_json::json!("automation");
    }
    // Record the spawner's tab for report-routing fallback.
    if let Ok(spawner) = std::env::var("CODETERM_TAB_ID") {
        if !spawner.is_empty() {
            body["spawned_by_tab_id"] = serde_json::json!(spawner);
        }
    }
    match client.call("agent.spawn", Some(body)) {
        Ok(val) => {
            if json {
                crate::cmd::print::print_json(&val);
                return;
            }
            let pid = val.get("tab_id").and_then(|v| v.as_str());
            let title_str = val.get("title").and_then(|v| v.as_str()).unwrap_or("?");
            let agent_str = val.get("agent").and_then(|v| v.as_str()).unwrap_or("?");
            let resolved = val
                .get("model")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|model| format!("{agent_str}/{model}"))
                .unwrap_or_else(|| agent_str.to_string());
            let warning = val.get("warning").and_then(|v| v.as_str());
            if let Some(pid) = pid {
                println!("Spawned {resolved}: {title_str} (Tab {pid})");
            } else if let Some(err) = val.get("error").or_else(|| val.get("message")) {
                eprintln!("Error: {}", err);
            } else {
                crate::cmd::print::print_object(&val);
            }
            if let Some(warning) = warning {
                eprintln!("Warning: {}", warning);
            }
            if let Some(hint) = crate::cmd::hints::maybe_orchestration_reminder() {
                println!("{hint}");
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_bound_worker_requires_an_exact_task_identity() {
        assert!(
            validate_spawn_context(Some(&SpawnRole::Worker), false, false, true, false, false)
                .is_err()
        );
        assert!(
            validate_spawn_context(Some(&SpawnRole::Worker), false, false, true, true, false)
                .is_ok()
        );
        assert!(
            validate_spawn_context(Some(&SpawnRole::Worker), false, false, true, false, true)
                .is_ok()
        );
        assert!(validate_spawn_context(
            Some(&SpawnRole::Worker),
            false,
            false,
            false,
            false,
            false
        )
        .is_ok());
        assert!(validate_spawn_context(None, false, false, true, false, false).is_ok());
    }

    #[test]
    fn team_spawn_fields_require_a_caller_tab() {
        let (team_id, caller) = team_spawn_fields(" team_alpha ", Some(" tab_manager ")).unwrap();
        assert_eq!(team_id, "team_alpha");
        assert_eq!(caller, "tab_manager");

        for caller in [None, Some(""), Some("   ")] {
            let error = team_spawn_fields("team_alpha", caller).unwrap_err();
            assert!(error.contains("CODETERM_TAB_ID"), "{error}");
        }
    }

    #[test]
    fn team_spawn_fields_reject_an_empty_team() {
        assert!(team_spawn_fields("  ", Some("tab_manager"))
            .unwrap_err()
            .contains("Team id"));
    }
}
