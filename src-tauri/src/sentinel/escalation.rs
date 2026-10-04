//! Reserved `other` handlers that escalate an unclassified observation and hold the stage.

use std::collections::BTreeSet;

use super::handlers::{self, Handler};
use super::machine::{EscalateTarget, Route};
use super::source::bounded_id;
use super::store::Sentinel;
use crate::storage::OTHER_BUCKET;

pub const MAX_ON_OTHER: usize = 4;

impl Sentinel {
    /// Handlers declared for `bucket`, including the reserved `other` bucket.
    pub fn declared_handlers(&self, bucket: &str) -> Option<&Vec<Handler>> {
        if bucket == OTHER_BUCKET {
            return (!self.on_other.is_empty()).then_some(&self.on_other);
        }
        self.routes.get(bucket)
    }
}

/// `other` handlers only notify or escalate, escalation appears nowhere else, and a hold names
/// an accepted `other` transition.
pub(super) fn validate(job: &Sentinel) -> Result<(), String> {
    let escalates_elsewhere = job
        .routes
        .values()
        .flatten()
        .any(|handler| matches!(handler.action, Route::Escalate(_)));
    if escalates_elsewhere
        || job.on_other.len() > MAX_ON_OTHER
        || job.on_other.is_empty() != job.escalation_timeout_ms.is_none()
        || job.escalation_timeout_ms == Some(0)
    {
        return Err("invalid sentinel escalation".into());
    }
    let mut ids: BTreeSet<&str> = job
        .routes
        .values()
        .flatten()
        .map(|handler| handler.id.as_str())
        .collect();
    for handler in &job.on_other {
        let scoped = match (&handler.action, &handler.target) {
            (Route::Notify | Route::Escalate(EscalateTarget::Creator), None) => true,
            (Route::Escalate(EscalateTarget::Agent(tab)), target) => {
                bounded_id(tab)
                    && target.as_ref().is_none_or(|target| {
                        target.kind == "worker"
                            && target.id == *tab
                            && target.workspace_id == job.workspace_id
                            && target.team_id == job.team_id
                    })
            }
            _ => false,
        };
        if !scoped || !handlers::valid_id(&handler.id) || !ids.insert(handler.id.as_str()) {
            return Err("invalid sentinel other handler".into());
        }
    }
    if let Some(held) = &job.machine.held {
        let accepted = job
            .transitions
            .iter()
            .any(|transition| transition.id == *held && transition.bucket == OTHER_BUCKET);
        if job.on_other.is_empty() || !accepted {
            return Err("invalid sentinel hold".into());
        }
    }
    Ok(())
}
