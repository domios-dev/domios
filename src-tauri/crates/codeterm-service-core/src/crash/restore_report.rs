pub use codeterm_agent_model::restore_report::*;
use std::collections::BTreeSet;

/// How the epoch that owned these panes ended, as one label. Restore losses
/// that are explained by a planned exit must not read as an unexplained crash,
/// and an unexplained one must not hide behind a plausible story.
pub fn previous_epoch_end_label() -> Option<String> {
    let state = crate::crash::lifecycle::recovery_state();
    if let Some(crash) = state.last_crash.as_ref() {
        return Some(format!("crash:{:?}", crash.classification));
    }
    state
        .last_clean_shutdown
        .as_ref()
        .map(|clean| format!("clean_shutdown:{:?}", clean.reason))
}

pub fn arm(bound: Vec<RestorePane>, restored_ids: &BTreeSet<String>) -> RestoreReport {
    arm_with_previous_epoch(bound, restored_ids, previous_epoch_end_label())
}
