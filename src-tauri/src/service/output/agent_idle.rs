use super::ServiceContext;

impl ServiceContext {
    pub(super) fn force_agent_idle_for_pty(&self, pty_id: &str) {
        let tab_id = self.mux.read().tab_id_for_pty(pty_id);
        if let Some(tab_id) = tab_id {
            self.force_agent_idle_for_tab(&tab_id);
        }
    }

    pub(super) fn force_agent_idle_for_tab(&self, tab_id: &str) {
        use crate::agent::offline_state_classifier::{AgentLiveState, TabVerdict, TimelineState};

        let idle_state: AgentLiveState = TabVerdict {
            state: TimelineState::Idle,
            subtype: None,
        }
        .into();
        let (changed, previous_identity, cleared_prompt) = {
            let mut mux = self.mux.write();
            match mux.get_tab_mut(tab_id) {
                Some(tab) if tab.agent_type.is_some() => {
                    let changed = tab.agent_state.as_ref() != Some(&idle_state)
                        || tab.pending_prompt.is_some();
                    let cleared_prompt = tab.pending_prompt.is_some();
                    let previous_identity =
                        crate::service::prompt_resolution::clear_tab_prompt(Some(tab));
                    tab.agent_state = Some(idle_state);
                    (changed, previous_identity, cleared_prompt)
                }
                _ => (false, None, false),
            }
        };
        if cleared_prompt {
            crate::service::launch_readiness::owner_prompt_cleared(tab_id);
            self.release_boot_owner_prompt(tab_id);
        }
        if let Some(identity) = previous_identity {
            self.resolve_prompt_notifications(tab_id, &identity);
        }
        if changed {
            self.observe_worker_idle_without_report(tab_id);
            self.nudge_stalled_teams_for_idle_orchestrator(tab_id);
            crate::service::folder_trust_carryover::tab_became_idle(self.clone(), tab_id);
            self.emit_state_changed();
            self.schedule_planner_replay(tab_id);
            self.heal_recovery_state_if_ready();
        }
    }
}
