use serde_json::{json, Value};
use tauri::AppHandle;

use crate::plan::Task;
use crate::service::plan::task_queries::TaskSection;
use crate::service::plan::{PlanError, PlanStore};
use crate::service::{ServiceState, SpawnAgentParams};

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

pub(super) struct DispatchRouting {
    pub agent: Option<String>,
    pub model: Option<String>,
}

/// Resolve the provider/model a dispatch should spawn. Precedence: an explicit
/// `agent` provider wins (any model rides along verbatim); otherwise a requested
/// model (RPC param, else the task's own routing field) that NAMES a registered
/// provider is treated as that provider at its default model; anything else is a
/// model name for the default provider. Never guesses a provider from a literal
/// id list — matches against the live registry.
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
        warnings.push(format!(
            "requested model '{}' is not available for provider '{}'; spawning with '{}' instead",
            notice.requested, resolved_provider, notice.using
        ));
    }

    // A personality is the executor persona: it names the pane and opens the brief.
    let title = detail
        .task
        .personality
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| detail.task.name.clone());

    let spawn_params = SpawnAgentParams {
        agent: routing.agent.clone(),
        cwd: owned_str(p, "cwd"),
        title: Some(title),
        role: Some("Worker".into()),
        parent_pane_id: parent,
        plan_id: Some(plan_id.clone()),
        task_id: Some(task_id.clone()),
        workspace_id: owned_str(p, "workspace_id"),
        model: routing.model.clone(),
        spawned_by_pane_id: owned_str(p, "caller_pane_id"),
        ..Default::default()
    };

    // Spawn first: a spawn failure leaves the task untouched — nothing to undo.
    let spawn = svc.spawn_agent_sync(spawn_params).map_err(spawn_failed)?;
    let pane_id = spawn.pane_id.clone();

    let outcome = (|| -> Result<(), JsonRpcError> {
        let assign = Some(json!({
            "plan_id": plan_id,
            "task_id": task_id,
            "pane": pane_id,
        }));
        assign_task_with_profile(store, svc, &assign, assign.as_ref(), now)?;
        set_in_progress(store, &plan_id, &task_id, now)?;
        let brief = build_brief(&detail.task, &detail.sections);
        svc.send_text(&pane_id, &brief, true)
            .map_err(|e| JsonRpcError {
                code: -32603,
                message: format!("brief delivery failed: {e}"),
                data: None,
            })?;
        Ok(())
    })();

    if let Err(e) = outcome {
        let _ = store.revert_task_dispatch(&plan_id, &task_id, now);
        let _ = svc.close_pane(&pane_id);
        emit_plan_changed(app, &plan_id);
        return Err(e);
    }

    emit_plan_changed(app, &plan_id);
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
    Ok(result)
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
    let mut out = String::new();
    let persona = task
        .personality
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let role_guidance = task
        .role_guidance
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match (persona, role_guidance) {
        (Some(persona), Some(guidance)) => {
            out.push_str(&format!("You are {persona}. {guidance}\n\n"));
        }
        (None, Some(guidance)) => {
            out.push_str(guidance);
            out.push_str("\n\n");
        }
        (Some(persona), None) => {
            out.push_str(&format!(
                "You are {persona} — executor for: {}\n\n",
                task.name
            ));
        }
        (None, None) => {}
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
        "## When done\nRun: codeterm agent report \"STATUS: done | blocked | partial\
         \\nCHANGES: ...\\nTESTS RUN: ...\"\n",
    );
    out
}

#[cfg(test)]
mod tests;
