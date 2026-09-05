use codeterm_types::team::staffing as team_codes;

use crate::args::agent::{AgentAction, SpawnRole};
use crate::client::SocketClient;

#[allow(clippy::too_many_arguments)]
pub(super) fn validate_spawn_context(
    role: Option<&SpawnRole>,
    has_workspace: bool,
    has_team: bool,
    has_parent: bool,
    has_plan: bool,
    has_bound_task: bool,
    has_plan_task: bool,
) -> Result<(), &'static str> {
    // A Team names the workspace its recovery Orchestrator lands in.
    if matches!(role, Some(SpawnRole::Orchestrator)) && !has_workspace && !has_parent && !has_team {
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

/// `--team` accepts exactly one role: the one that anchors a Team. Anything
/// else takes its Team from its own binding and is refused by code.
pub(super) fn validate_team_spawn_role(role: Option<&SpawnRole>) -> Result<(), String> {
    match role {
        None | Some(SpawnRole::Orchestrator) => Ok(()),
        Some(other) => Err(format!(
            "{}: --team accepts --role Orchestrator only, got {}. A role-bound agent takes its Team from that binding.",
            team_codes::TEAM_SPAWN_ROLE_MUST_ANCHOR,
            other.as_str()
        )),
    }
}

/// The one `--team` + `--role` pair that is not a contradiction. It leaves the
/// `agent.spawn` request entirely and calls the Team staffing verb instead, so
/// the CLI and the UI add-member action reach the daemon the same way.
pub(super) fn routes_to_team_staffing(role: Option<&SpawnRole>, team: Option<&str>) -> bool {
    matches!(role, Some(SpawnRole::Orchestrator)) && team.is_some()
}

/// Flags the Team staffing verb does not carry. Naming them is the refusal —
/// dropping a flag the caller set would be a silent lie about what was spawned.
pub(super) fn unsupported_team_orchestrator_flags(
    present: &[(&'static str, bool)],
) -> Vec<&'static str> {
    present
        .iter()
        .filter(|(_, set)| *set)
        .map(|(name, _)| *name)
        .collect()
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
        team.is_some(),
        parent.is_some(),
        plan.is_some(),
        bound_task.is_some(),
        plan_task.is_some(),
    ) {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
    if team.is_some() {
        if let Err(error) = validate_team_spawn_role(role.as_ref()) {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    }
    let team_orchestrator = team
        .as_deref()
        .filter(|team_id| routes_to_team_staffing(role.as_ref(), Some(*team_id)));
    if let Some(team_id) = team_orchestrator {
        let unsupported = unsupported_team_orchestrator_flags(&[
            ("--description", description.is_some()),
            ("--cwd", cwd.is_some()),
            ("--parent", parent.is_some()),
            ("--plan", plan.is_some()),
            ("--report-to", report_to.is_some()),
            ("--can-spawn", can_spawn),
            ("--args", args.is_some()),
            ("--workspace", workspace.is_some()),
            ("--reasoning-effort", reasoning_effort.is_some()),
            ("--system", system_prompt.is_some()),
            ("--preset", preset.is_some()),
            ("--charter", charter.is_some() || charter_file.is_some()),
            ("--interval", interval.is_some()),
            ("--nudge-cooldown", nudge_cooldown.is_some()),
            ("--max-actions", max_actions.is_some()),
            ("--task-id", bound_task.is_some()),
            ("--non-user-facing", non_user_facing),
        ]);
        if !unsupported.is_empty() {
            eprintln!(
                "Error: {}: --team --role Orchestrator does not carry {}",
                team_codes::TEAM_SPAWN_FLAG_UNSUPPORTED,
                unsupported.join(", ")
            );
            std::process::exit(1);
        }
        let task = match crate::cmd::agent::resolve_spawn_task(
            task,
            task_file,
            None,
            &mut std::io::stdin(),
        ) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        };
        add_team_orchestrator(
            client,
            json,
            &agent,
            team_id,
            crate::cmd::agent::join_cli_words(title),
            task,
            model,
        );
        return;
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
    let is_worker = matches!(role, Some(crate::args::agent::SpawnRole::Worker));
    for hint in crate::cmd::hints::worker_spawn_hints(
        is_worker,
        parent.is_some() || report_to.is_some(),
        task.is_some(),
    ) {
        eprintln!("{hint}");
    }
    let worker_bound = crate::cmd::report_guard::worker_binding_resolved(
        plan_task.is_some(),
        plan.is_some(),
        bound_task.is_some(),
    );
    if let Some(hint) = crate::cmd::hints::unbound_worker_binding_hint(is_worker, worker_bound) {
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

/// Hand a Team a new Orchestrator through the Team staffing verb — the same
/// daemon entry the UI add-member action uses. Deliberately caller-less: a Team
/// whose Orchestrator is gone has no membership left for a caller to spend.
fn add_team_orchestrator(
    client: &mut SocketClient,
    json: bool,
    agent: &str,
    team_id: &str,
    title: Option<String>,
    task: Option<String>,
    model: Option<String>,
) {
    let mut body = serde_json::json!({
        "team_id": team_id,
        "provider": agent,
        "role": "orchestrator",
    });
    if let Some(title) = title {
        body["title"] = serde_json::json!(title);
    }
    if let Some(task) = task {
        body["task"] = serde_json::json!(task);
    }
    if let Some(model) = model {
        body["model"] = serde_json::json!(model);
    }
    match client.call("team.add_member", Some(body)) {
        Ok(val) => {
            if json {
                crate::cmd::print::print_json(&val);
                return;
            }
            match val.get("tab_id").and_then(|v| v.as_str()) {
                Some(tab_id) => println!("Orchestrator added to Team {team_id} (Tab {tab_id})"),
                None => crate::cmd::print::print_object(&val),
            }
            let takeover = val.get("plan_takeover");
            let plan_id = takeover
                .and_then(|t| t.get("plan_id"))
                .and_then(|v| v.as_str());
            let takeover_error = takeover
                .and_then(|t| t.get("error"))
                .and_then(|v| v.as_str());
            match (plan_id, takeover_error) {
                (Some(plan_id), None) => println!("Plan {plan_id} now owned by the new Orchestrator"),
                (Some(plan_id), Some(error)) => {
                    eprintln!("WARNING: plan {plan_id} ownership did not move: {error}")
                }
                _ => {}
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
    use crate::cmd::hints::unbound_worker_binding_hint;
    use crate::cmd::report_guard::worker_binding_resolved;

    /// codeterm#305: an unbound Worker spawn is accepted (docs/SKILLS.md's
    /// ad-hoc pattern) but carries the shared advisory constant.
    #[test]
    fn worker_without_plan_binding_is_refused_or_warned_at_spawn() {
        assert!(validate_spawn_context(
            Some(&SpawnRole::Worker),
            false,
            false,
            false,
            false,
            false,
            false
        )
        .is_ok());
        let bound = worker_binding_resolved(false, false, false);
        assert!(!bound);
        assert!(unbound_worker_binding_hint(true, bound).is_some());
    }

    #[test]
    fn plan_bound_worker_requires_an_exact_task_identity() {
        assert!(validate_spawn_context(
            Some(&SpawnRole::Worker),
            false,
            false,
            false,
            true,
            false,
            false
        )
        .is_err());
        assert!(validate_spawn_context(
            Some(&SpawnRole::Worker),
            false,
            false,
            false,
            true,
            true,
            false
        )
        .is_ok());
        assert!(validate_spawn_context(
            Some(&SpawnRole::Worker),
            false,
            false,
            false,
            true,
            false,
            true
        )
        .is_ok());
        assert!(validate_spawn_context(
            Some(&SpawnRole::Worker),
            false,
            false,
            false,
            false,
            false,
            false
        )
        .is_ok());
        assert!(validate_spawn_context(None, false, false, false, true, false, false).is_ok());
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

    /// codeterm#418: `--team --role Orchestrator` is the recovery add and needs
    /// no `--workspace`; the Team names the workspace.
    #[test]
    fn team_supplies_the_workspace_for_an_orchestrator_spawn() {
        assert!(validate_spawn_context(
            Some(&SpawnRole::Orchestrator),
            false,
            true,
            false,
            false,
            false,
            false
        )
        .is_ok());
        assert!(validate_spawn_context(
            Some(&SpawnRole::Orchestrator),
            false,
            false,
            false,
            false,
            false,
            false
        )
        .is_err());
    }

    #[test]
    fn team_spawn_accepts_only_the_anchoring_role() {
        assert!(validate_team_spawn_role(None).is_ok());
        assert!(validate_team_spawn_role(Some(&SpawnRole::Orchestrator)).is_ok());
        for role in [
            SpawnRole::Worker,
            SpawnRole::Planner,
            SpawnRole::Manager,
            SpawnRole::Watcher,
        ] {
            let error = validate_team_spawn_role(Some(&role))
                .expect_err("a non-anchoring role takes its Team from its own binding");
            assert!(
                error.starts_with(team_codes::TEAM_SPAWN_ROLE_MUST_ANCHOR),
                "the refusal must lead with its code, got: {error}"
            );
        }
    }

    /// The CLI recovery add leaves `agent.spawn` and lands on the same daemon
    /// staffing verb the UI add-member action uses.
    #[test]
    fn only_an_anchoring_role_with_a_team_routes_to_team_staffing() {
        assert!(routes_to_team_staffing(
            Some(&SpawnRole::Orchestrator),
            Some("team_alpha")
        ));
        assert!(!routes_to_team_staffing(Some(&SpawnRole::Orchestrator), None));
        assert!(!routes_to_team_staffing(None, Some("team_alpha")));
        assert!(!routes_to_team_staffing(
            Some(&SpawnRole::Worker),
            Some("team_alpha")
        ));
    }

    #[test]
    fn unsupported_recovery_flags_are_named_not_dropped() {
        assert!(unsupported_team_orchestrator_flags(&[("--cwd", false)]).is_empty());
        assert_eq!(
            unsupported_team_orchestrator_flags(&[
                ("--cwd", true),
                ("--preset", false),
                ("--workspace", true),
            ]),
            vec!["--cwd", "--workspace"]
        );
    }
}
