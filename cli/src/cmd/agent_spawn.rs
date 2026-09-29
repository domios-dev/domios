use codeterm_types::team::staffing as team_codes;

use crate::args::agent::{AgentAction, SpawnRole};
use crate::client::SocketClient;

/// The flag combination an `agent spawn` was invoked with, as the validator
/// needs to see it. A bag of bare booleans stopped being readable once the
/// task-less verification Worker gave `--execution-intent` a meaning of its own.
#[derive(Default)]
pub(super) struct SpawnContext<'a> {
    pub role: Option<&'a SpawnRole>,
    pub has_workspace: bool,
    pub has_team: bool,
    pub has_parent: bool,
    pub has_plan: bool,
    pub has_bound_task: bool,
    pub has_plan_task: bool,
    pub unplanned: bool,
    pub execution_intent: Option<&'a str>,
    pub has_authorized_gates: bool,
}

impl SpawnContext<'_> {
    fn is_worker(&self) -> bool {
        matches!(self.role, Some(SpawnRole::Worker))
    }

    fn task_bound(&self) -> bool {
        self.has_bound_task || self.has_plan_task
    }

    fn read_only(&self) -> bool {
        self.execution_intent == Some(READ_ONLY)
    }

    /// A Worker that owns no task and declares read-only intent: the shape an
    /// Orchestrator staffs post-merge QA with. It runs the commands it declared
    /// and, holding no binding, can transition nothing.
    pub(super) fn is_verification_worker(&self) -> bool {
        self.is_worker() && !self.task_bound() && self.read_only()
    }
}

const READ_ONLY: &str = "read_only";
const MUTATING: &str = "mutating";

/// Both supported task-less Worker shapes, named in every refusal that turns a
/// caller away — an error that only demands `--plan-task` hides the other one.
const WORKER_SHAPES: &str = concat!(
    "a Worker that changes source owns a task (--plan-task <task>, or --plan <id> ",
    "--task-id <task>); a Worker that only verifies owns none and declares ",
    "--execution-intent read_only, optionally with repeated --authorized-gate <command>"
);

pub(super) fn validate_spawn_context(ctx: &SpawnContext<'_>) -> Result<(), String> {
    // A Team names the workspace its recovery Orchestrator lands in.
    if matches!(ctx.role, Some(SpawnRole::Orchestrator))
        && !ctx.has_workspace
        && !ctx.has_parent
        && !ctx.has_team
    {
        return Err("--workspace is required when spawning --role Orchestrator".into());
    }
    if matches!(ctx.role, Some(SpawnRole::Planner)) && !ctx.has_plan && !ctx.has_parent {
        return Err(
            "--plan <id> is required when spawning --role Planner (or pass --parent)".into(),
        );
    }
    if ctx.has_authorized_gates {
        if !ctx.is_worker() {
            return Err("--authorized-gate is only valid with --role Worker".into());
        }
        if ctx.has_plan_task {
            return Err(concat!(
                "--authorized-gate is not carried by a task-bound spawn; declare the gates ",
                "where they are also enforced: codeterm plan task dispatch <plan> <task> ",
                "--authorized-gate <command>"
            )
            .into());
        }
    }
    // Read-only intent is the exit both refusals below name, so it has to be
    // the exit both of them honour.
    if ctx.is_worker() && !ctx.task_bound() && !ctx.read_only() && !ctx.unplanned {
        if ctx.execution_intent == Some(MUTATING) {
            return Err(format!(
                "--execution-intent mutating requires a task binding: {WORKER_SHAPES}"
            ));
        }
        if ctx.has_plan {
            return Err(format!(
                "--role Worker with --plan requires a task binding or read-only intent: \
                 {WORKER_SHAPES}. Omit --role/--plan for a roleless solo agent."
            ));
        }
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
        unplanned,
        reason,
        team,
        execution_intent,
        authorized_gates,
        provider_reserve_pct,
        non_user_facing,
    } = action
    else {
        unreachable!("agent_spawn::handle received a non-spawn action")
    };
    let context = SpawnContext {
        role: role.as_ref(),
        has_workspace: workspace.is_some(),
        has_team: team.is_some(),
        has_parent: parent.is_some(),
        has_plan: plan.is_some(),
        has_bound_task: bound_task.is_some(),
        has_plan_task: plan_task.is_some(),
        unplanned,
        execution_intent: execution_intent.as_deref(),
        has_authorized_gates: !authorized_gates.is_empty(),
    };
    if let Err(error) = validate_spawn_context(&context) {
        crate::cmd::print::fail(1, error.to_string());
    }
    if unplanned {
        let reason = reason
            .as_deref()
            .map(str::trim)
            .filter(|reason| !reason.is_empty());
        if !matches!(role.as_ref(), Some(SpawnRole::Worker))
            || reason.is_none()
            || execution_intent
                .as_deref()
                .is_some_and(|intent| intent != MUTATING)
            || bound_task.is_some()
            || plan_task.is_some()
            || team.is_some()
            || !authorized_gates.is_empty()
        {
            crate::cmd::print::fail(2, "--unplanned requires --role Worker and a non-empty --reason, and cannot combine with task bindings, --team, or --authorized-gate");
        }
    } else if reason.is_some() {
        crate::cmd::print::fail(2, "--reason requires --unplanned");
    }
    let verification_worker = context.is_verification_worker();
    if team.is_some() {
        if let Err(error) = validate_team_spawn_role(role.as_ref()) {
            crate::cmd::print::fail(1, error.to_string());
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
            crate::cmd::print::fail(
                1,
                format!(
                    "{}: --team --role Orchestrator does not carry {}",
                    team_codes::TEAM_SPAWN_FLAG_UNSUPPORTED,
                    unsupported.join(", ")
                ),
            );
        }
        let task = match crate::cmd::agent::resolve_spawn_task(
            task,
            task_file,
            None,
            &mut std::io::stdin(),
        ) {
            Ok(t) => t,
            Err(e) => {
                crate::cmd::print::fail(1, e.to_string());
            }
        };
        add_team_orchestrator(
            client,
            json,
            agent.as_deref().unwrap_or("claude"),
            team_id,
            crate::cmd::agent::join_cli_words(title),
            task,
            model,
        );
        return;
    }
    let task = match crate::cmd::agent::resolve_spawn_task(
        task,
        task_file,
        cwd.as_deref(),
        &mut std::io::stdin(),
    ) {
        Ok(t) => t,
        Err(e) => {
            crate::cmd::print::fail(1, e.to_string());
        }
    };
    if unplanned {
        let mut params = serde_json::json!({
            "agent": agent,
            "role": "Worker",
            "reason": reason.expect("validated above"),
            "brief": task.unwrap_or_default(),
            "execution_intent": "mutating",
            "caller_tab_id": std::env::var("CODETERM_TAB_ID").ok(),
            "session_id": std::env::var("CODETERM_SESSION_ID").ok(),
            "session_binding_nonce": std::env::var("CODETERM_SESSION_BINDING_NONCE").ok(),
        });
        if let Some(plan) = plan.or_else(|| std::env::var("CODETERM_PLAN_ID").ok()) {
            params["plan_id"] = serde_json::json!(plan);
        }
        if let Some(parent) = parent {
            params["parent_tab_id"] = serde_json::json!(parent);
        }
        if let Some(workspace) = workspace {
            params["workspace_id"] = serde_json::json!(workspace);
        }
        if let Some(model) = model {
            params["model"] = serde_json::json!(model);
        }
        match client.call_detailed("plan.task.dispatch_unplanned", Some(params)) {
            Ok(value) => {
                if json {
                    crate::cmd::print::print_json(&value);
                } else {
                    println!("Dispatched unplanned Worker task {}", value["task_id"]);
                }
            }
            Err(error) => crate::cmd::model_error::exit_with_socket_error(json, error),
        }
        return;
    }
    let charter = match crate::cmd::agent::resolve_spawn_charter(
        charter,
        charter_file,
        &mut std::io::stdin(),
    ) {
        Ok(t) => t,
        Err(e) => {
            crate::cmd::print::fail(1, e.to_string());
        }
    };
    let interval = match interval
        .as_deref()
        .map(|v| crate::cmd::agent::parse_duration_secs(v, "--interval"))
        .transpose()
    {
        Ok(v) => v,
        Err(e) => {
            crate::cmd::print::fail(1, e.to_string());
        }
    };
    let nudge_cooldown = match nudge_cooldown
        .as_deref()
        .map(|v| crate::cmd::agent::parse_duration_secs(v, "--nudge-cooldown"))
        .transpose()
    {
        Ok(v) => v,
        Err(e) => {
            crate::cmd::print::fail(1, e.to_string());
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
    if let Some(hint) =
        crate::cmd::hints::unbound_worker_binding_hint(is_worker, worker_bound, verification_worker)
    {
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
                crate::cmd::print::fail(1, e.to_string());
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
                crate::cmd::print::fail(1, error.to_string());
            }
        }
    }
    if let Some(intent) = execution_intent {
        body["execution_intent"] = serde_json::json!(intent);
        body["provider_reserve_pct"] = serde_json::json!(provider_reserve_pct);
    }
    if !authorized_gates.is_empty() {
        body["authorized_gates"] = serde_json::json!(authorized_gates);
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
    match client.call_detailed("agent.spawn", Some(body)) {
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
                crate::cmd::print::fail(1, err.to_string());
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
            crate::cmd::model_error::exit_with_socket_error(json, e);
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
    match client.call_detailed("team.add_member", Some(body)) {
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
                (Some(plan_id), None) => {
                    println!("Plan {plan_id} now owned by the new Orchestrator")
                }
                (Some(plan_id), Some(error)) => {
                    eprintln!("WARNING: plan {plan_id} ownership did not move: {error}")
                }
                _ => {}
            }
            if let Some(warning) = val.get("warning").and_then(|value| value.as_str()) {
                eprintln!("Warning: {warning}");
            }
        }
        Err(e) => {
            crate::cmd::model_error::exit_with_socket_error(json, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::hints::unbound_worker_binding_hint;
    use crate::cmd::report_guard::worker_binding_resolved;

    fn worker(execution_intent: Option<&'static str>, has_plan: bool) -> SpawnContext<'static> {
        SpawnContext {
            role: Some(&SpawnRole::Worker),
            has_plan,
            execution_intent,
            ..Default::default()
        }
    }

    /// codeterm#305: an unbound Worker spawn is accepted (prod-docs/SKILLS.md's
    /// ad-hoc pattern) but carries the shared advisory constant.
    #[test]
    fn worker_without_plan_binding_is_refused_or_warned_at_spawn() {
        assert!(validate_spawn_context(&worker(None, false)).is_ok());
        let bound = worker_binding_resolved(false, false, false);
        assert!(!bound);
        assert!(unbound_worker_binding_hint(true, bound, false).is_some());
    }

    #[test]
    fn plan_bound_worker_requires_an_exact_task_identity() {
        assert!(validate_spawn_context(&worker(None, true)).is_err());
        assert!(validate_spawn_context(&SpawnContext {
            has_bound_task: true,
            ..worker(None, true)
        })
        .is_ok());
        assert!(validate_spawn_context(&SpawnContext {
            has_plan_task: true,
            ..worker(None, true)
        })
        .is_ok());
        assert!(validate_spawn_context(&worker(None, false)).is_ok());
        assert!(validate_spawn_context(&SpawnContext {
            has_plan: true,
            ..Default::default()
        })
        .is_ok());
    }

    /// The verification Worker: read-only, owns no task, and is accepted with
    /// or without the plan it was spawned under.
    #[test]
    fn a_read_only_worker_is_accepted_without_any_task_binding() {
        for has_plan in [false, true] {
            let context = worker(Some(READ_ONLY), has_plan);
            assert!(
                validate_spawn_context(&context).is_ok(),
                "read-only Worker with has_plan={has_plan} must be accepted"
            );
            assert!(context.is_verification_worker());
        }
    }

    /// The safeguard that stays: source-changing work still owns a task.
    #[test]
    fn a_mutating_worker_without_a_task_is_refused_and_names_both_shapes() {
        for has_plan in [false, true] {
            let error = validate_spawn_context(&worker(Some(MUTATING), has_plan))
                .expect_err("a mutating Worker owns a task");
            assert!(error.contains("--plan-task"), "{error}");
            assert!(error.contains("--execution-intent read_only"), "{error}");
        }
        assert!(validate_spawn_context(&SpawnContext {
            has_plan_task: true,
            ..worker(Some(MUTATING), true)
        })
        .is_ok());
    }

    /// A refusal that only demanded the flag left the other supported shape
    /// invisible, which is how a verifier had nowhere to be spawned from.
    #[test]
    fn the_plan_bound_worker_refusal_names_the_read_only_shape() {
        let error = validate_spawn_context(&worker(None, true))
            .expect_err("--plan without a task binding is refused");
        assert!(error.contains("--execution-intent read_only"), "{error}");
        assert!(error.contains("--authorized-gate"), "{error}");
        assert!(error.contains("--plan-task"), "{error}");
    }

    #[test]
    fn declared_gates_belong_to_a_task_less_worker_only() {
        assert!(validate_spawn_context(&SpawnContext {
            has_authorized_gates: true,
            ..worker(Some(READ_ONLY), true)
        })
        .is_ok());

        let roleless = validate_spawn_context(&SpawnContext {
            has_authorized_gates: true,
            ..Default::default()
        })
        .expect_err("gates are a Worker concept");
        assert!(roleless.contains("--role Worker"), "{roleless}");

        let task_bound = validate_spawn_context(&SpawnContext {
            has_authorized_gates: true,
            has_plan_task: true,
            ..worker(Some(READ_ONLY), true)
        })
        .expect_err("a task-bound spawn declares its gates on the dispatch");
        assert!(task_bound.contains("plan task dispatch"), "{task_bound}");
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
        assert!(validate_spawn_context(&SpawnContext {
            role: Some(&SpawnRole::Orchestrator),
            has_team: true,
            ..Default::default()
        })
        .is_ok());
        assert!(validate_spawn_context(&SpawnContext {
            role: Some(&SpawnRole::Orchestrator),
            ..Default::default()
        })
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
        assert!(!routes_to_team_staffing(
            Some(&SpawnRole::Orchestrator),
            None
        ));
        assert!(!routes_to_team_staffing(None, Some("team_alpha")));
        assert!(!routes_to_team_staffing(
            Some(&SpawnRole::Worker),
            Some("team_alpha")
        ));
    }

    /// The provisioning Worker staffed on a mesh peer: it changes source, but
    /// `plan task dispatch` is local-only, so no binding exists for it to own.
    /// That shape is admitted, and the mutating declaration — the one case
    /// where a binding is possible and therefore required — still is not.
    #[test]
    fn a_mesh_worker_owns_no_task_and_is_still_admitted() {
        validate_spawn_context(&worker(None, false))
            .expect("a Worker with no plan and no declared intent is a supported shape");

        let refused = validate_spawn_context(&worker(Some(MUTATING), false))
            .expect_err("declaring mutating intent asks for a binding");
        assert!(refused.contains("--plan-task"), "{refused}");
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
