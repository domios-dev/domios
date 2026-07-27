use super::ServiceContext;
use crate::agent::registry::AgentRegistry;
use crate::agent::status::TabStatus;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::types::Tab;
use crate::AgentTrackerState;

fn reconcile_tab_status(registry: &AgentRegistry, tab: &Tab, raw: TabStatus) -> TabStatus {
    let deterministic_agent_state = tab
        .agent_type
        .as_deref()
        .and_then(|agent_type| {
            registry.with_provider(agent_type, |provider| {
                provider.supports_agent_state_status_reconciliation()
            })
        })
        .unwrap_or(false);
    codeterm_types::pty::reconcile_status(
        raw,
        tab.boot_stage,
        tab.agent_state.as_ref().map(|state| state.state),
        deterministic_agent_state,
        tab.pending_prompt.is_some(),
    )
}

pub(super) fn snapshot_with_reconciled_status(
    mux: &MuxState,
    tracker: &AgentTrackerState,
    registry: &AgentRegistry,
) -> crate::multiplexer::state::MultiplexerSnapshot {
    let mux = mux.read();
    let tracker = tracker.lock();
    mux.snapshot_with_status_resolver(&tracker, |tab, raw| {
        reconcile_tab_status(registry, tab, raw)
    })
}

impl ServiceContext {
    fn reconcile_tab_status(&self, tab: &Tab, raw: TabStatus) -> TabStatus {
        reconcile_tab_status(&self.agent_registry, tab, raw)
    }

    pub(crate) fn reconciled_tab_status_for_id(
        &self,
        tab_id: &str,
        raw: TabStatus,
    ) -> TabStatus {
        self.mux
            .read()
            .get_tab(tab_id)
            .map(|tab| self.reconcile_tab_status(tab, raw))
            .unwrap_or(raw)
    }

    pub(crate) fn snapshot_with_reconciled_status(
        &self,
    ) -> crate::multiplexer::state::MultiplexerSnapshot {
        snapshot_with_reconciled_status(&self.mux, &self.agent_tracker, &self.agent_registry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_hold::{TimedMutex, TimedRwLock};
    use crate::multiplexer::state::MultiplexerState;
    use crate::plugin::agent_claude::ClaudeProvider;
    use codeterm_types::pty::{BootStage, TabStatusTracker, TabVerdict, TimelineState};
    use std::sync::Arc;

    #[test]
    fn snapshot_and_live_projection_agree_for_ready_prompt_repaint() {
        let (mut state, _root) = MultiplexerState::new();
        let workspace_id = state.list_workspaces()[0].id.clone();
        let tab_id = state.create_tab(&workspace_id).unwrap();
        {
            let tab = state.get_tab_mut(&tab_id).unwrap();
            tab.pty_id = Some("pty-repaint".into());
            tab.agent_type = Some("claude".into());
            tab.boot_stage = Some(BootStage::AgentReady);
            tab.agent_state = Some(
                TabVerdict {
                    state: TimelineState::Idle,
                    subtype: None,
                }
                .into(),
            );
        }

        let mux: MuxState = Arc::new(TimedRwLock::new("status-reconciliation-test", state));
        let mut raw_tracker = TabStatusTracker::new();
        raw_tracker.update_output("pty-repaint", [1u8; 32]);
        let tracker: AgentTrackerState =
            Arc::new(TimedMutex::new("status-reconciliation-test", raw_tracker));
        let registry = AgentRegistry::new();
        registry.register_provider(Box::new(ClaudeProvider::new()));

        let live_status = {
            let mux = mux.read();
            reconcile_tab_status(
                &registry,
                mux.get_tab(&tab_id).unwrap(),
                TabStatus::Working,
            )
        };
        let snapshot = snapshot_with_reconciled_status(&mux, &tracker, &registry);

        assert_eq!(live_status, TabStatus::Idle);
        assert_eq!(snapshot.tab_statuses.get(&tab_id), Some(&live_status));
    }
}
