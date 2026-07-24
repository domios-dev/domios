use super::ServiceContext;
use crate::agent::registry::AgentRegistry;
use crate::agent::status::PaneStatus;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::types::Pane;
use crate::AgentTrackerState;

fn reconcile_pane_status(registry: &AgentRegistry, pane: &Pane, raw: PaneStatus) -> PaneStatus {
    let deterministic_agent_state = pane
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
        pane.boot_stage,
        pane.agent_state.as_ref().map(|state| state.state),
        deterministic_agent_state,
        pane.pending_prompt.is_some(),
    )
}

pub(super) fn snapshot_with_reconciled_status(
    mux: &MuxState,
    tracker: &AgentTrackerState,
    registry: &AgentRegistry,
) -> crate::multiplexer::state::MultiplexerSnapshot {
    let mux = mux.read();
    let tracker = tracker.lock();
    mux.snapshot_with_status_resolver(&tracker, |pane, raw| {
        reconcile_pane_status(registry, pane, raw)
    })
}

impl ServiceContext {
    fn reconcile_pane_status(&self, pane: &Pane, raw: PaneStatus) -> PaneStatus {
        reconcile_pane_status(&self.agent_registry, pane, raw)
    }

    pub(crate) fn reconciled_pane_status_for_id(
        &self,
        pane_id: &str,
        raw: PaneStatus,
    ) -> PaneStatus {
        self.mux
            .read()
            .get_pane(pane_id)
            .map(|pane| self.reconcile_pane_status(pane, raw))
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
    use codeterm_types::pty::{BootStage, PaneStatusTracker, PaneVerdict, TimelineState};
    use std::sync::Arc;

    #[test]
    fn snapshot_and_live_projection_agree_for_ready_prompt_repaint() {
        let (mut state, _root) = MultiplexerState::new();
        let workspace_id = state.list_workspaces()[0].id.clone();
        let (_tab_id, pane_id) = state.create_tab(&workspace_id).unwrap();
        {
            let pane = state.get_pane_mut(&pane_id).unwrap();
            pane.pty_id = Some("pty-repaint".into());
            pane.agent_type = Some("claude".into());
            pane.boot_stage = Some(BootStage::AgentReady);
            pane.agent_state = Some(
                PaneVerdict {
                    state: TimelineState::Idle,
                    subtype: None,
                }
                .into(),
            );
        }

        let mux: MuxState = Arc::new(TimedRwLock::new("status-reconciliation-test", state));
        let mut raw_tracker = PaneStatusTracker::new();
        raw_tracker.update_output("pty-repaint", [1u8; 32]);
        let tracker: AgentTrackerState =
            Arc::new(TimedMutex::new("status-reconciliation-test", raw_tracker));
        let registry = AgentRegistry::new();
        registry.register_provider(Box::new(ClaudeProvider::new()));

        let live_status = {
            let mux = mux.read();
            reconcile_pane_status(
                &registry,
                mux.get_pane(&pane_id).unwrap(),
                PaneStatus::Working,
            )
        };
        let snapshot = snapshot_with_reconciled_status(&mux, &tracker, &registry);

        assert_eq!(live_status, PaneStatus::Idle);
        assert_eq!(snapshot.pane_statuses.get(&pane_id), Some(&live_status));
    }
}
