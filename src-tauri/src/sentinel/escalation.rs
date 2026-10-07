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
pub(crate) fn validate(job: &Sentinel) -> Result<(), String> {
    let escalates_elsewhere = job
        .routes
        .values()
        .flatten()
        .any(|handler| matches!(handler.action, Route::Escalate(_)));
    let escalation_count = job
        .on_other
        .iter()
        .filter(|handler| matches!(&handler.action, Route::Escalate(_)))
        .count();
    if escalates_elsewhere
        || job.on_other.len() > MAX_ON_OTHER
        || escalation_count > 1
        || job.on_other.is_empty() != job.escalation_timeout_ms.is_none()
        || job.escalation_timeout_ms == Some(0)
    {
        return Err("invalid blueprint escalation".into());
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
            return Err("invalid blueprint other handler".into());
        }
    }
    if let Some(held) = &job.machine.held {
        let accepted = job
            .transitions
            .iter()
            .any(|transition| transition.id == *held && transition.bucket == OTHER_BUCKET);
        if escalation_count == 0 || !accepted {
            return Err("invalid blueprint hold".into());
        }
    }
    Ok(())
}

pub fn expiry_at(job: &Sentinel) -> Option<u64> {
    let held = job.machine.held.as_ref()?;
    let accepted_at = job
        .transitions
        .iter()
        .find(|transition| transition.id == *held)?
        .accepted_at;
    Some(accepted_at.saturating_add(job.escalation_timeout_ms?))
}
