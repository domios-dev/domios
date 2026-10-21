use super::*;
use crate::service::supporter;

pub(crate) fn journal(
    runtime: &supporter::SupporterRuntime,
    decision: &Decision,
    plan: &str,
    now: u64,
) {
    let event = decision.journal_event(&decision.scope.tab_id, plan, now);
    if let Err(error) = runtime.heartbeat_event(event) {
        log::warn!("[heartbeat-gate] journal unavailable: {error}");
    }
}
