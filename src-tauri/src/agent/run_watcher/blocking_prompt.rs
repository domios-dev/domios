use std::sync::atomic::Ordering;

use crate::service::ServiceContext;

use super::{finish_run_as, AgentRunKind, RunEnd, RunState};

pub(super) fn tab_pending_prompt_kind(
    svc: &ServiceContext,
    tab_id: &str,
) -> Option<codeterm_types::pty::PromptKind> {
    svc.mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.pending_prompt_kind)
}

pub(super) fn should_end_on_blocking_prompt(
    kind: &AgentRunKind,
    provider_prompt_pending: bool,
    pending_prompt_kind: Option<codeterm_types::pty::PromptKind>,
) -> bool {
    provider_prompt_pending
        && matches!(kind, AgentRunKind::MemWorker { .. })
        && matches!(
            pending_prompt_kind,
            Some(
                codeterm_types::pty::PromptKind::Provider
                    | codeterm_types::pty::PromptKind::Structured
                    | codeterm_types::pty::PromptKind::DirectoryTrust
            )
        )
}

pub(super) fn claim_run_end(state: &RunState) -> bool {
    !state.completed.swap(true, Ordering::AcqRel)
}

pub(super) fn end_on_blocking_prompt(svc: &ServiceContext, state: &RunState) {
    let summary = match tab_pending_prompt_kind(svc, &state.tab_id) {
        Some(codeterm_types::pty::PromptKind::DirectoryTrust) => "folder_trust",
        _ => codeterm_mem::dream_status::deadline::APPROVAL_PROMPT_REASON,
    }
    .to_string();
    log::warn!(
        "[run-watcher] ending mem worker on blocking prompt: tab={} reason={}",
        state.tab_id,
        summary
    );
    finish_run_as(svc, state, RunEnd::TimedOut, summary.clone());
    if let AgentRunKind::MemWorker { worker_id } = &state.kind {
        crate::mem::dream_runs::finalize_dream_at_deadline(svc, worker_id, &summary);
    }
    svc.run_watcher.runs.lock().remove(&state.pty_id);
}
