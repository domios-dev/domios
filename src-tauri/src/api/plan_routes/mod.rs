use std::time::{SystemTime, UNIX_EPOCH};

use axum::{http::StatusCode, Json};
use serde_json::Value;

use crate::api::routes::AppState;
use crate::plan::{tag_facets_from_value, MilestoneKind, TaskGroup};
use crate::service::plan::PlanError;

pub mod attachments;
pub mod batch;
pub mod elements;
pub mod gates;
pub mod handoff;
pub mod linking;
pub mod loops;
pub mod milestones;
pub mod plans;
pub mod sections;
pub mod skills;
pub mod spec;
pub mod tasks;

pub use attachments::attachment;
pub use elements::{
    coverage, element_add, element_rm, element_update, groups_set, layout_set, note_add,
    note_update,
};
pub use gates::{gate_answer, gate_ask, gate_escalate, gate_list};
pub use linking::{link, unlink};
pub use loops::{loop_create, loop_record, loop_set, loop_status, loop_stop};
pub use milestones::{milestone_add, milestone_rm, milestone_update};
pub use plans::{
    archive, claim, create, delete_plan, disown, get, kinds_set, list, mode_set, next, pause,
    reconcile, resolve_refs, resume, state, tag_facets_set, transfer, unarchive, update,
};
pub use sections::{ingest, section_add, section_rm, section_update};
pub use skills::{retrospective_write, skills_list, skills_show};
pub use tasks::{
    task_add, task_assign, task_attempt_show, task_attempts, task_close, task_respawn, task_rm,
    task_update,
};

#[cfg(test)]
mod tests;

type PlanRouteResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn text_err(status: StatusCode, message: impl ToString) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(serde_json::json!({ "error": message.to_string() })),
    )
}

fn plan_err(e: PlanError) -> (StatusCode, Json<Value>) {
    let status = match e {
        PlanError::NotFound | PlanError::TaskNotFound => StatusCode::NOT_FOUND,
        PlanError::WritePending { .. } => StatusCode::SERVICE_UNAVAILABLE,
        PlanError::ReadbackMismatch { .. } => StatusCode::CONFLICT,
        _ => StatusCode::BAD_REQUEST,
    };
    let body = serde_json::json!({
        "error": e.to_string(),
        "plan_error": e.to_json(),
    });
    (status, Json(body))
}

fn emit_plan_changed(state: &AppState, plan_id: &str) {
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "plan-changed",
        serde_json::json!({ "plan_id": plan_id }),
    );
}

fn optional_string_field<'a>(body: &'a Value, key: &str) -> Option<Option<&'a str>> {
    body.get(key).map(|v| v.as_str())
}

fn kinds_from_csv(raw: &str) -> Vec<MilestoneKind> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|key| MilestoneKind {
            key: key.into(),
            label: key.into(),
            color: None,
        })
        .collect()
}

fn groups_from_csv(raw: &str) -> Vec<TaskGroup> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|item| {
            let (key, label) = item.split_once(':').unwrap_or((item, item));
            TaskGroup {
                key: key.trim().into(),
                label: label.trim().into(),
                color: None,
            }
        })
        .collect()
}

fn tag_facets_from_body(value: &Value) -> Result<Vec<crate::plan::TagFacet>, String> {
    tag_facets_from_value(value)
}
