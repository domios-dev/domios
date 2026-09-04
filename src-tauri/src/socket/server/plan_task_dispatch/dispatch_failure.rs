use serde_json::{json, Value};

use crate::service::worker_ack::{ATTEMPT_STATE_FINALIZED, TAB_STATE_ALIVE_AWAITING_INSPECTION};

use super::{dispatch_failed, JsonRpcError};

pub(in crate::socket::server) enum DispatchWaitFailure {
    Uncertain(String),
    Definitive(String),
}

impl DispatchWaitFailure {
    pub(in crate::socket::server) fn message(&self) -> &str {
        match self {
            Self::Uncertain(message) | Self::Definitive(message) => message,
        }
    }

    pub(in crate::socket::server) fn is_definitive(&self) -> bool {
        matches!(self, Self::Definitive(_))
    }
}

pub(super) fn dispatch_wait_failed(stage: &str, failure: DispatchWaitFailure) -> JsonRpcError {
    let worker_may_be_alive = !failure.is_definitive();
    let mut error = dispatch_failed(stage, failure.message().to_string());
    if let Some(data) = error.data.as_mut() {
        data["worker_may_be_alive"] = json!(worker_may_be_alive);
    }
    error
}

#[derive(Clone, Copy)]
pub(super) enum DispatchFailureDisposition {
    Survivor { attempt_finalized: bool },
    Cleanup { ok: bool },
}

pub(super) fn dispatch_startup_failed_event(
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    attempt_id: &str,
    stage: &str,
    error: &str,
    disposition: DispatchFailureDisposition,
) -> Value {
    let mut event = json!({
        "kind": "dispatch_startup_failed",
        "plan_id": plan_id,
        "task_id": task_id,
        "tab_id": tab_id,
        "attempt_id": attempt_id,
        "stage": stage,
        "error": error,
    });
    match disposition {
        DispatchFailureDisposition::Survivor { attempt_finalized } => {
            event["tab_state"] = json!(TAB_STATE_ALIVE_AWAITING_INSPECTION);
            event["attempt_state"] = json!(if attempt_finalized {
                ATTEMPT_STATE_FINALIZED
            } else {
                "not_current"
            });
        }
        DispatchFailureDisposition::Cleanup { ok } => {
            event["cleanup"] = json!({"ok": ok});
        }
    }
    event
}
