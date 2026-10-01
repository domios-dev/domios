//! App-side seam between a worker's finalization report and the durable diary
//! record that report names. Reconciliation reads the stored record; it never
//! rewrites one, reruns a graph mutation, or invents final content.

use codeterm_mem::dream_status::{
    diary_evidence, reconcile_finalization, DiaryEvidence, DiaryFinalization, DiaryRecordState,
    FinalizationClaim,
};

use crate::service::ServiceContext;

/// What a worker explicitly stated about its own artifact when it reported.
#[derive(Debug, Default, Clone)]
pub struct ReportedFinalization {
    pub diary_id: Option<String>,
    pub claim: Option<String>,
    pub reason: Option<String>,
}

impl ReportedFinalization {
    pub fn is_empty(&self) -> bool {
        self.diary_id.is_none() && self.claim.is_none() && self.reason.is_none()
    }
}

fn trimmed(value: Option<&String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Read the named diary back out of the store so finalization is judged on the
/// record that exists, not on what the run said about it.
pub fn evidence_for_diary(svc: &ServiceContext, diary_id: &str) -> Option<DiaryEvidence> {
    match svc.mem.get(diary_id) {
        Ok(Some(item)) => Some(diary_evidence(diary_id, &item.body)),
        Ok(None) => None,
        Err(error) => {
            log::warn!("[dreaming] diary {} lookup failed: {}", diary_id, error);
            None
        }
    }
}

/// Record the reconciled finalization fact against exactly one worker.
pub fn record_reported_finalization(
    svc: &ServiceContext,
    worker_id: &str,
    reported: &ReportedFinalization,
) -> Option<DiaryFinalization> {
    if reported.is_empty() {
        return None;
    }
    let diary_id = trimmed(reported.diary_id.as_ref());
    let claim = trimmed(reported.claim.as_ref()).and_then(|raw| FinalizationClaim::from_str(&raw));
    let reason = trimmed(reported.reason.as_ref());
    let evidence = diary_id
        .as_deref()
        .and_then(|id| evidence_for_diary(svc, id));

    let finalization = reconcile_finalization(claim, reason.as_deref(), evidence.as_ref());
    if let Err(error) = svc
        .workers
        .record_finalization(worker_id, diary_id, finalization.clone())
    {
        log::warn!(
            "[dreaming] finalization not recorded for {}: {}",
            worker_id,
            error
        );
        return None;
    }
    Some(finalization)
}

/// Present one stored diary record: complete, live work, or recoverable
/// history. Liveness is resolved from the worker registry by diary identity.
pub fn diary_record_state(svc: &ServiceContext, diary_id: &str, body: &str) -> DiaryRecordState {
    let evidence = diary_evidence(diary_id, body);
    let live_owner = svc.workers.live_owner_of_diary(diary_id);
    codeterm_mem::dream_status::classify_diary_record(&evidence, live_owner.as_deref())
}

/// Finalize a deadline-ended dreaming run host-side; a no-op for other workers.
/// Blocking store IO: call it off the async runtime.
pub fn finalize_dream_at_deadline(svc: &ServiceContext, worker_id: &str, detail: &str) {
    let dreaming = svc
        .workers
        .get(worker_id)
        .is_some_and(|w| crate::mem::workers::is_dreaming_intent(&w.intent));
    if !dreaming {
        return;
    }
    match codeterm_mem::dream_status::deadline::finalize_at_deadline(
        &svc.mem,
        &svc.workers,
        worker_id,
        detail,
    ) {
        Ok(outcome) => log::info!(
            "[dreaming] deadline finalization for {}: diary={:?} rewritten={} recorded={}",
            worker_id,
            outcome.diary_id,
            outcome.diary_rewritten,
            outcome.finalization_recorded
        ),
        Err(error) => log::warn!(
            "[dreaming] deadline finalization failed for {}: {}",
            worker_id,
            error
        ),
    }
}
