//! The authority boundary every plan batch write crosses.
//!
//! Both external entrances to `PlanStore::apply_batch` — the `plan.batch` socket
//! verb and `POST /api/plans/{id}/batch` — funnel through this module, because a
//! bearer token proves only that a client is permitted, never that it still holds
//! the task it names. The report path is the third caller and declares the
//! authority it already established, rather than being exempt by not appearing on
//! a list; an exemption by omission is how the next entrance arrives unguarded.

use serde_json::{json, Value};

use crate::service::hierarchy::report_plan_ctx::STALE_TASK_HINT_REFUSAL;
use crate::service::plan::batch::{BatchOp, BatchRequest, BatchResult};
use crate::service::plan::{BatchTaskRefusalRead, PlanError, PlanStore};

mod completion;

/// Machine-readable discriminant for the one refusal class, carried as
/// `data.type` on the socket and `plan_error.type` over HTTP.
pub const STALE_TASK_BINDING_TYPE: &str = "stale_task_binding";

/// Why this caller may write. Every entrance states one; there is no default.
pub enum BatchAuthority<'a> {
    /// Identified only by the tab id it sent, so every task it names is checked.
    Caller { tab_id: Option<&'a str> },
    /// Established upstream by a path that already authenticated the caller and
    /// verified its hold on the task; `established_by` names that path.
    EstablishedUpstream { established_by: &'static str },
}

/// The refused attempt, named on both sides so "who set this task done?" stays
/// answerable from the caller's own error and from the daemon log alike.
pub struct BatchRefusal {
    pub message: String,
    pub(crate) attempting_tab: Option<String>,
    pub(crate) plan_id: String,
    pub(crate) task_id: String,
}

impl BatchRefusal {
    pub fn to_json(&self) -> Value {
        json!({
            "type": STALE_TASK_BINDING_TYPE,
            "attempted_by_tab": self.attempting_tab,
            "plan_id": self.plan_id,
            "task_id": self.task_id,
        })
    }
}

pub enum BatchWriteError {
    Refused(BatchRefusal),
    Plan(PlanError),
}

/// The `plan.batch` socket verb carries caller identity in the RPC params.
pub fn apply_batch_from_socket(
    store: &PlanStore,
    plan_id: &str,
    params: Option<&Value>,
    request: BatchRequest,
    now_ms: u64,
) -> Result<BatchResult, BatchWriteError> {
    let tab_id = params
        .and_then(|value| value.get("caller_tab_id"))
        .and_then(Value::as_str);
    apply_batch_with_authority(
        store,
        plan_id,
        request,
        now_ms,
        BatchAuthority::Caller { tab_id },
    )
}

/// `POST /api/plans/{id}/batch` carries it in the JSON body instead. `check_auth`
/// upstream proves the client is permitted, which is a different question.
pub fn apply_batch_from_http(
    store: &PlanStore,
    plan_id: &str,
    body: &Value,
    request: BatchRequest,
    now_ms: u64,
) -> Result<BatchResult, BatchWriteError> {
    let tab_id = body.get("caller_tab_id").and_then(Value::as_str);
    apply_batch_with_authority(
        store,
        plan_id,
        request,
        now_ms,
        BatchAuthority::Caller { tab_id },
    )
}

pub fn apply_batch_with_authority(
    store: &PlanStore,
    plan_id: &str,
    request: BatchRequest,
    now_ms: u64,
    authority: BatchAuthority<'_>,
) -> Result<BatchResult, BatchWriteError> {
    match authority {
        BatchAuthority::Caller { tab_id } => {
            let tab_id = tab_id.map(str::trim).filter(|tab| !tab.is_empty());
            authorize_caller(store, plan_id, &request, tab_id)?;
        }
        BatchAuthority::EstablishedUpstream { established_by } => log::debug!(
            "[plan/authority] batch write on {plan_id} authorized upstream by {established_by}"
        ),
    }
    store
        .apply_batch(plan_id, request, now_ms)
        .map_err(BatchWriteError::Plan)
}

fn authorize_caller(
    store: &PlanStore,
    plan_id: &str,
    request: &BatchRequest,
    tab_id: Option<&str>,
) -> Result<(), BatchWriteError> {
    let selectors = request
        .ops
        .iter()
        .filter_map(op_task_selector)
        .filter(|selector| !selector.starts_with('$'))
        .map(str::to_string)
        .collect::<Vec<_>>();
    if let Some(task) = store
        .batch_task_refusal_projection(plan_id, &selectors, tab_id)
        .map_err(BatchWriteError::Plan)?
    {
        return Err(BatchWriteError::Refused(
            refuse(store, plan_id, &task, tab_id).map_err(BatchWriteError::Plan)?,
        ));
    }
    Ok(())
}

/// The task each op mutates. A `$ref` names an id minted earlier in the same
/// batch, whose own op is checked on its way past, so it is not a second door.
fn op_task_selector(op: &BatchOp) -> Option<&str> {
    let selector = match op {
        BatchOp::UpsertTask { handle, .. } => handle.as_str(),
        BatchOp::SetTaskBody { task, .. }
        | BatchOp::SetTaskSections { task, .. }
        | BatchOp::SetTaskStatus { task, .. }
        | BatchOp::AddMilestone { task, .. } => task.as_str(),
        BatchOp::AddHandoff {
            task: Some(task), ..
        } => task.as_str(),
        _ => return None,
    };
    Some(selector)
}

fn holder_label(task: &BatchTaskRefusalRead) -> String {
    match task.assigned_tab_id.as_deref().map(str::trim) {
        Some(tab) if !tab.is_empty() => format!("tab '{tab}'"),
        _ => "an open competition attempt".to_string(),
    }
}

/// Names both sides of the mismatch to the caller and leaves the same fact in the
/// daemon log, because a silent no-op only replaces a silent corruption.
fn refuse(
    store: &PlanStore,
    plan_id: &str,
    task: &BatchTaskRefusalRead,
    tab_id: Option<&str>,
) -> Result<BatchRefusal, PlanError> {
    let caller = tab_id.unwrap_or("<no caller_tab_id>");
    let bindings = tab_id
        .map(|tab| store.task_bindings_with_name_for_tab(tab))
        .transpose()?
        .unwrap_or_default();
    let held = if bindings.is_empty() {
        "holds no live task binding".to_string()
    } else {
        format!(
            "holds {}",
            bindings
                .iter()
                .map(|(bound_plan, bound_task, name)| format!("{bound_plan}/{bound_task} «{name}»"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let task_id = task.task_id.clone();
    let message = format!(
        "{STALE_TASK_HINT_REFUSAL}: tab '{caller}' tried to write task '{task_id}' «{}» in plan \
         '{plan_id}', which is held by {}, but that tab {held}. Nothing was written to \
         '{task_id}' — no status, no body, no sections, no milestone. Record from the tab that \
         holds this task, or as the plan owner; a shell carrying a task id the daemon has moved \
         must drop it and let the daemon resolve its live binding.",
        task.name,
        holder_label(task),
    );
    log::warn!(
        "[plan/authority] {STALE_TASK_HINT_REFUSAL}: tab '{caller}' attempted a batch write to \
         {plan_id}/{task_id} it does not hold; refused, nothing written"
    );
    Ok(BatchRefusal {
        message,
        attempting_tab: tab_id.map(str::to_string),
        plan_id: plan_id.to_string(),
        task_id,
    })
}
