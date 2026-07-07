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
    let spawn_params = SpawnAgentParams {
        agent: owned_str(p, "agent"),
        cwd: owned_str(p, "cwd"),
        title: Some(detail.task.name.clone()),
        role: Some("Worker".into()),
        parent_pane_id: parent,
        plan_id: Some(plan_id.clone()),
        task_id: Some(task_id.clone()),
        workspace_id: owned_str(p, "workspace_id"),
        model: owned_str(p, "model"),
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
        svc.send_text(&pane_id, &brief, true).map_err(|e| JsonRpcError {
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
    Ok(json!({
        "ok": true,
        "plan_id": plan_id,
        "task_id": task_id,
        "pane_id": pane_id,
        "spawn": spawn,
    }))
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
    let mut out = format!("# {}\n\n", task.name);

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
