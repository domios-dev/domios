use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::ServiceContext;
use crate::agent::registry::{AgentRegistry, ProviderStateSnapshot};
use crate::agent::status::TabStatus;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::types::Tab;
use crate::AgentTrackerState;

/// Stable, observable refusal used when an authoritative snapshot would have
/// to wait for a lifecycle owner. Callers must wait for the next state event;
/// this path never turns a readiness wait into a blocking lock acquisition.
pub const SNAPSHOT_BUSY_ERROR: &str =
    "state snapshot unavailable while tab state is initializing or changing";

fn reconcile_tab_status(providers: &ProviderStateSnapshot, tab: &Tab, raw: TabStatus) -> TabStatus {
    let deterministic_agent_state = providers.reconciles_status(tab.agent_type.as_deref());
    hold_running_shell_command(
        tab,
        codeterm_types::pty::reconcile_status(
            raw,
            tab.boot_stage,
            tab.agent_state.as_ref().map(|state| state.state),
            deterministic_agent_state,
            tab.pending_prompt.is_some(),
        ),
    )
}

/// Output silence never ends an open shell-marker command span.
fn hold_running_shell_command(tab: &Tab, status: TabStatus) -> TabStatus {
    let running = tab.is_utility_shell()
        && tab.shell_command_state.activity == crate::multiplexer::types::ShellActivity::Running;
    if running && status == TabStatus::Idle {
        TabStatus::Working
    } else {
        status
    }
}

pub(super) fn snapshot_with_reconciled_status(
    mux: &MuxState,
    tracker: &AgentTrackerState,
    registry: &AgentRegistry,
) -> crate::multiplexer::state::MultiplexerSnapshot {
    let providers = registry.state_snapshot();
    let mux = mux.read();
    let tracker = tracker.lock();
    mux.snapshot_with_status_resolver(&tracker, |tab, raw| {
        reconcile_tab_status(&providers, tab, raw)
    })
}

pub(super) fn try_reconciled_tab_statuses(
    mux: &MuxState,
    tracker: &AgentTrackerState,
    registry: &AgentRegistry,
) -> Option<std::collections::HashMap<String, TabStatus>> {
    let providers = registry.state_snapshot();
    let mux = mux.try_read()?;
    let tracker = tracker.try_lock()?;
    Some(
        mux.all_tabs()
            .filter_map(|tab| {
                crate::multiplexer::state::resolved_tab_status(tab, &tracker, |tab, raw| {
                    reconcile_tab_status(&providers, tab, raw)
                })
                .map(|status| (tab.id.clone(), status))
            })
            .collect(),
    )
}

pub(crate) fn try_snapshot_with_reconciled_status(
    mux: &MuxState,
    tracker: &AgentTrackerState,
    registry: &AgentRegistry,
) -> Option<crate::multiplexer::state::MultiplexerSnapshot> {
    // Preserve the blocking snapshot's lock order while refusing immediately
    // if startup or a lifecycle transition owns either lock.
    let providers = registry.state_snapshot();
    let mux = mux.try_read()?;
    let tracker = tracker.try_lock()?;
    Some(mux.snapshot_with_status_resolver(&tracker, |tab, raw| {
        reconcile_tab_status(&providers, tab, raw)
    }))
}

/// The reconciled snapshot every tab status surface reads, waiting for the
/// state locks until `deadline` instead of refusing on a concurrent reader.
pub(crate) fn snapshot_with_reconciled_status_until(
    mux: &MuxState,
    tracker: &AgentTrackerState,
    registry: &AgentRegistry,
    deadline: std::time::Instant,
) -> Option<crate::multiplexer::state::MultiplexerSnapshot> {
    let providers = registry.state_snapshot();
    crate::multiplexer::state::snapshot_with_status_until(mux, tracker, deadline, |tab, raw| {
        reconcile_tab_status(&providers, tab, raw)
    })
}

/// Blocks until both snapshot locks are free, taken in the snapshot's lock order.
pub(crate) fn wait_until_snapshot_readable(mux: &MuxState, tracker: &AgentTrackerState) {
    let _mux = mux.read();
    let _tracker = tracker.lock();
}

static SNAPSHOT_RELEASE_WAKE_ARMED: AtomicBool = AtomicBool::new(false);

impl ServiceContext {
    /// Keeps a refusal's promise of a later state event, including when the
    /// lock owner itself never emits one (a session save, for example).
    pub fn emit_state_changed_when_snapshot_readable(self: &Arc<Self>, cause: super::MuxEmitCause) {
        if SNAPSHOT_RELEASE_WAKE_ARMED.swap(true, Ordering::AcqRel) {
            return;
        }
        let svc = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("mux-release-wake".into())
            .spawn(move || {
                wait_until_snapshot_readable(&svc.mux, &svc.agent_tracker);
                SNAPSHOT_RELEASE_WAKE_ARMED.store(false, Ordering::Release);
                svc.emit_state_changed(cause);
            });
        if let Err(error) = spawned {
            SNAPSHOT_RELEASE_WAKE_ARMED.store(false, Ordering::Release);
            log::error!("[events] snapshot release wake spawn failed: {error}");
        }
    }

    /// One tab's status as every surface reports it. The caller already holds
    /// `mux`; `agent_tracker` is taken second, per lock order.
    pub fn resolved_tab_status(&self, tab: &Tab) -> Option<TabStatus> {
        let providers = self.agent_registry.state_snapshot();
        let tracker = self.agent_tracker.lock();
        crate::multiplexer::state::resolved_tab_status(tab, &tracker, |tab, raw| {
            reconcile_tab_status(&providers, tab, raw)
        })
    }

    pub fn reconciled_tab_status_for_id(&self, tab_id: &str, raw: TabStatus) -> TabStatus {
        let providers = self.agent_registry.state_snapshot();
        self.mux
            .read()
            .get_tab(tab_id)
            .map(|tab| reconcile_tab_status(&providers, tab, raw))
            .unwrap_or(raw)
    }

    pub(crate) fn snapshot_with_reconciled_status(
        &self,
    ) -> crate::multiplexer::state::MultiplexerSnapshot {
        snapshot_with_reconciled_status(&self.mux, &self.agent_tracker, &self.agent_registry)
    }

    pub fn try_snapshot_with_reconciled_status(
        &self,
    ) -> Option<crate::multiplexer::state::MultiplexerSnapshot> {
        try_snapshot_with_reconciled_status(&self.mux, &self.agent_tracker, &self.agent_registry)
    }
}

#[cfg(test)]
mod tests;
