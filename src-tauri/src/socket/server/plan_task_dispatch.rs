use serde_json::{json, Value};
use tauri::AppHandle;

use crate::plan::Task;
use crate::service::plan::milestone_gates::{
    regression_dispatch_decision, DispatchExemption, RegressionDispatchDecision, RegressionGate,
};
use crate::service::plan::task_queries::TaskSection;
use crate::service::plan::{PlanError, PlanStore};
use crate::service::{
    ProvisionedWorkerLane, ServiceState, SpawnAgentParams, SpawnIdentityProvenance,
};

use super::plan::{emit_plan_changed, plan_rpc_err};
use super::plan_refs;
use super::plan_role_helpers::assign_task_with_profile;
use super::JsonRpcError;

fn owned_str(p: Option<&Value>, key: &str) -> Option<String> {
    p.and_then(|x| x.get(key))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn spawn_failed(message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message,
        data: None,
    }
}

fn gate_keys(gates: &[RegressionGate]) -> String {
    gates
        .iter()
        .map(|g| g.key.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

// Refuse the dispatch loudly, naming every blocking gate — no pane is spawned.
fn regression_blocked_error(gates: Vec<RegressionGate>) -> JsonRpcError {
    let names = gate_keys(&gates);
    JsonRpcError {
        code: -32010,
        message: format!(
            "dispatch blocked by pending regression gate(s): {names}. Run the warm integration QA to clear them (codeterm plan qa next <plan> --group <gate> --note \"<evidence>\"), tag the task regression-gate/release-blocking if it is the exception, or record an authorized override (codeterm plan qa override <plan> --reason \"<why>\")."
        ),
        data: Some(json!({
            "type": "regression_gate_blocked",
            "blocking_gates": gates,
        })),
    }
}

pub(super) struct DispatchRouting {
    pub agent: Option<String>,
    pub model: Option<String>,
}

fn qualified_provider_model(model: &str, provider_ids: &[String]) -> Option<(String, String)> {
    for separator in [':', '/'] {
        let Some((provider, model)) = model.split_once(separator) else {
            continue;
        };
        if provider_ids
            .iter()
            .any(|registered| registered.eq_ignore_ascii_case(provider))
            && !model.trim().is_empty()
        {
            return Some((provider.to_string(), model.to_string()));
        }
    }
    None
}

fn unqualified_model_error(
    routing: &DispatchRouting,
    provider: &str,
    notice: &crate::service::spawn::ModelFallbackNotice,
) -> Option<JsonRpcError> {
    if routing.agent.is_some() {
        return None;
    }
    Some(JsonRpcError {
        code: -32602,
        message: format!(
            "model '{}' is unavailable for the default provider '{}'. Specify an explicit provider/model instead of silently substituting '{}'.",
            notice.requested_model, provider, notice.provider_resolved_model
        ),
        data: Some(json!({
            "type": "unqualified_model_unavailable",
            "requested_model": notice.requested_model,
            "default_provider": provider,
            "suggestion": "use <provider>:<model> or pass --agent <provider>",
        })),
    })
}

fn dispatch_worker_identity(task: &Task) -> (Option<String>, Option<SpawnIdentityProvenance>) {
    let title = task
        .personality
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let provenance = if title.is_some() {
        match task.personality_provenance {
            Some(crate::plan::IdentityProvenance::UserSupplied) => {
                Some(SpawnIdentityProvenance::UserSupplied)
            }
            _ => Some(SpawnIdentityProvenance::Suggested),
        }
    } else {
        Some(SpawnIdentityProvenance::Suggested)
    };
    (title, provenance)
}

/// Resolve the provider/model a dispatch should spawn. Precedence: an explicit
/// `agent` provider wins (any model rides along verbatim); otherwise a requested
/// provider/model prefix is resolved against the live registry. A bare provider
/// name selects its default model; a bare model remains a request for the default
/// provider and is rejected if that provider cannot resolve it.
pub(super) fn resolve_dispatch_routing(
    explicit_agent: Option<String>,
    rpc_model: Option<String>,
    task_model: Option<String>,
    provider_ids: &[String],
) -> DispatchRouting {
    let clean = |s: Option<String>| s.filter(|v| !v.trim().is_empty());
    let requested_model = clean(rpc_model).or_else(|| clean(task_model));
    if let Some(agent) = clean(explicit_agent) {
        return DispatchRouting {
            agent: Some(agent),
            model: requested_model,
        };
    }
    if let Some(model) = requested_model {
        if let Some((agent, model)) = qualified_provider_model(&model, provider_ids) {
            return DispatchRouting {
                agent: Some(agent),
                model: Some(model),
            };
        }
        if provider_ids.iter().any(|p| p.eq_ignore_ascii_case(&model)) {
            return DispatchRouting {
                agent: Some(model),
                model: None,
            };
        }
        return DispatchRouting {
            agent: None,
            model: Some(model),
        };
    }
    DispatchRouting {
        agent: None,
        model: None,
    }
}

/// Atomic: spawn a Worker pane, assign the task (in_progress + lane metadata),
/// then deliver the brief. Any post-spawn failure reverts the task and closes
/// the pane, so a task is never left in_progress without a live pane.
pub(super) fn dispatch(
    params: &Option<Value>,
    app: &AppHandle,
    svc: &ServiceState,
    now: u64,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let p = params.as_ref();
    let (plan_id, task_id) = plan_refs::resolve_task(store, params)?;
    let detail = store
        .show_task(&task_id)
        .ok_or_else(|| plan_rpc_err(PlanError::TaskNotFound))?;

    // Frontier policy: a non-blocker cannot spawn while regression gates are
    // pending. Decide BEFORE any pane is created so a block leaves no orphan.
    let plan = store
        .get(&plan_id)
        .ok_or_else(|| plan_rpc_err(PlanError::NotFound))?;
    let mut exemption: Option<DispatchExemption> = None;
    match regression_dispatch_decision(&plan, &detail.task) {
        RegressionDispatchDecision::Clear => {}
        RegressionDispatchDecision::Exempt { exemption: e, .. } => exemption = Some(e),
        RegressionDispatchDecision::Blocked { blocking } => {
            return Err(regression_blocked_error(blocking));
        }
    }
    // Same frontier policy for the review WIP gate: decide before any pane
    // exists. A task resuming after a bounce already holds its slot.
    if detail.task.status == crate::plan::TaskStatus::Pending {
        let gate = store.review_gate_status(&plan_id).map_err(plan_rpc_err)?;
        if gate.blocked {
            return Err(plan_rpc_err(PlanError::ReviewWipExceeded {
                wip_count: gate.wip_count,
                wip_limit: gate.wip_limit,
                credit: gate.credit,
            }));
        }
    }

    // Parent defaults to the dispatching (caller) pane, resolved server-side.
    let parent = owned_str(p, "parent_pane_id").or_else(|| owned_str(p, "caller_pane_id"));
    let routing = resolve_dispatch_routing(
        owned_str(p, "agent"),
        owned_str(p, "model"),
        detail.task.model.clone(),
        &svc.agent_registry.provider_ids(),
    );

    // Surface an unknown-model fallback loudly instead of silently downgrading.
    let resolved_provider = routing
        .agent
        .clone()
        .unwrap_or_else(crate::config::default_agent_provider);
    let mut warnings: Vec<String> = Vec::new();
    if let Some(notice) = crate::service::spawn::resolve_fresh_agent_model_with_notice(
        &svc.agent_registry,
        &resolved_provider,
        routing.model.clone(),
        None,
    )
    .fallback_notice
    {
        if let Some(error) = unqualified_model_error(&routing, &resolved_provider, &notice) {
            return Err(error);
        }
        warnings.push(format!(
            "requested model '{}' is not available for provider '{}'; spawning with '{}' instead",
            notice.requested_model, resolved_provider, notice.provider_resolved_model
        ));
    }

    // A personality is the executor persona: it names the pane and opens the brief.
    let (explicit_persona, title_provenance) = dispatch_worker_identity(&detail.task);

    let mut spawn_params = SpawnAgentParams {
        agent: routing.agent.clone(),
        cwd: owned_str(p, "cwd"),
        title: explicit_persona,
        title_provenance,
        role: Some("Worker".into()),
        parent_pane_id: parent,
        plan_id: Some(plan_id.clone()),
        task_id: Some(task_id.clone()),
        workspace_id: owned_str(p, "workspace_id"),
        model: routing.model.clone(),
        planned_model: detail.task.model.clone(),
        reasoning_effort: owned_str(p, "reasoning_effort"),
        spawned_by_pane_id: owned_str(p, "caller_pane_id"),
        ..Default::default()
    };

    let lane_label = format!("{plan_id}-{task_id}");
    let lane = svc
        .provision_worker_lane_for_spawn(&mut spawn_params, &lane_label)
        .map_err(spawn_failed)?;
    // Spawn first: a spawn failure leaves the task untouched — nothing to undo.
    let spawn = match svc.spawn_agent_sync(spawn_params) {
        Ok(spawn) => spawn,
        Err(error) => {
            if let Some(lane) = lane {
                lane.cleanup();
            }
            return Err(spawn_failed(error));
        }
    };
    let pane_id = spawn.pane_id.clone();

    let outcome = (|| -> Result<(), JsonRpcError> {
        let assign = Some(json!({
            "plan_id": plan_id,
            "task_id": task_id,
            "pane": pane_id,
        }));
        assign_task_with_profile(store, svc, &assign, assign.as_ref(), now)?;
        set_in_progress(store, &plan_id, &task_id, now)?;
        let session_id = svc
            .mux
            .read()
            .get_pane(&pane_id)
            .and_then(|pane| pane.agent_session_id.clone());
        store
            .update_attempt_executor_identity(
                &plan_id,
                &task_id,
                &pane_id,
                Some(&spawn.title),
                session_id.as_deref(),
                now,
            )
            .map_err(plan_rpc_err)?;
        let brief = build_brief_with_claimed_persona(&detail.task, &detail.sections, &spawn.title);
        svc.send_text_detached(&pane_id, &brief, true);
        Ok(())
    })();

    if let Err(e) = outcome {
        rollback_failed_dispatch(
            lane,
            || {
                let _ = store.revert_task_dispatch(&plan_id, &task_id, now);
            },
            || {
                let _ = svc.close_pane(&pane_id);
            },
        );
        emit_plan_changed(app, &plan_id);
        return Err(e);
    }

    emit_plan_changed(app, &plan_id);
    let model_fallback = spawn.model_fallback.clone();
    let mut result = json!({
        "ok": true,
        "plan_id": plan_id,
        "task_id": task_id,
        "pane_id": pane_id,
        "agent": resolved_provider,
        "spawn": spawn,
    });
    if !warnings.is_empty() {
        result["warnings"] = json!(warnings);
    }
    if let Some(exemption) = exemption {
        result["regression_exception"] = json!({
            "exemption": exemption,
            "reason": exemption.reason(),
        });
    }
    if let Some(model_fallback) = model_fallback {
        result["model_fallback"] = json!(model_fallback);
    }
    Ok(result)
}

fn rollback_failed_dispatch<R, C>(
    lane: Option<ProvisionedWorkerLane>,
    revert_task: R,
    close_pane: C,
) where
    R: FnOnce(),
    C: FnOnce(),
{
    revert_task();
    close_pane();
    if let Some(lane) = lane {
        lane.cleanup();
    }
}

fn build_brief_with_claimed_persona(
    task: &Task,
    sections: &[TaskSection],
    claimed_title: &str,
) -> String {
    let mut claimed_task = task.clone();
    claimed_task.personality = Some(claimed_title.to_string());
    build_brief(&claimed_task, sections)
}

fn set_in_progress(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    now: u64,
) -> Result<(), JsonRpcError> {
    store
        .update_task(
            plan_id,
            task_id,
            None,
            None,
            None,
            None,
            Some("in_progress"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            now,
        )
        .map_err(plan_rpc_err)
}

/// Assemble the worker brief: body, acceptance, test plan, section links,
/// strides (milestones — T4 enriches later), and the mandatory report command.
pub(super) fn build_brief(task: &Task, sections: &[TaskSection]) -> String {
    build_brief_with_personality(task, sections, task.personality.as_deref())
}

pub(super) fn build_brief_with_personality(
    task: &Task,
    sections: &[TaskSection],
    effective_personality: Option<&str>,
) -> String {
    let mut out = String::new();
    if let Some(identity) = crate::service::plan::worker_identity::compose_worker_identity(
        effective_personality,
        task.role_guidance.as_deref(),
        Some(task.name.as_str()),
    ) {
        out.push_str(&identity);
        out.push_str("\n\n");
    }
    out.push_str(&format!("# {}\n\n", task.name));

    let body = task.body.trim();
    if !body.is_empty() {
        out.push_str(body);
        out.push_str("\n\n");
    }

    if !task.acceptance_criteria.is_empty() {
        out.push_str("## Acceptance criteria\n");
        for c in &task.acceptance_criteria {
            out.push_str(&format!("- {c}\n"));
        }
        out.push('\n');
    }

    if let Some(tp) = task
        .test_plan
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        out.push_str(&format!("## Test plan\n{tp}\n\n"));
    }

    if !sections.is_empty() {
        out.push_str("## Sections\n");
        for s in sections {
            out.push_str(&format!("- {} ({})\n", s.title, s.id));
        }
        out.push('\n');
    }

    if !task.milestones.is_empty() {
        out.push_str("## Strides\n");
        for m in &task.milestones {
            out.push_str(&format!("- [{}] {}\n", m.kind, m.text));
        }
        out.push('\n');
    }

    out.push_str(
        "## When done\nRun: codeterm agent report \"STATUS: done | blocked | partial\\nCHANGES: ...\\nTESTS RUN: ...\"\n",
    );
    out
}

#[cfg(test)]
mod tests;
