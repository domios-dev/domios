use crate::agent::status::TabStatus;

use super::ServiceContext;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StopDecision {
    NoAgent,
    NotWorking(TabStatus),
    Interrupt {
        agent_id: String,
        session_id: Option<String>,
    },
}

impl ServiceContext {
    /// A stop key reaches the provider only while a turn is in progress; sent
    /// to an idle agent it opens provider UI modes instead of stopping anything.
    pub(crate) fn stop_decision(&self, tab_id: &str) -> StopDecision {
        let mux = self.mux.read();
        let Some(tab) = mux.get_tab(tab_id) else {
            return StopDecision::NoAgent;
        };
        let Some(agent_id) = tab.agent_type.clone() else {
            return StopDecision::NoAgent;
        };
        let status =
            crate::multiplexer::state::effective_tab_status(tab, self.resolved_tab_status(tab));
        if !status.has_active_turn() {
            return StopDecision::NotWorking(status);
        }
        StopDecision::Interrupt {
            agent_id,
            session_id: tab.agent_session_id.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StopDecision;
    use crate::agent::status::TabStatus;
    use crate::multiplexer::state::MultiplexerState;

    #[test]
    #[cfg_attr(
        not(windows),
        ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
    )]
    fn stop_idle_agent_is_noop() {
        let (mut mux, tab_id) = MultiplexerState::new();
        {
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.pty_id = Some("pty-stop".into());
            tab.agent_type = Some("opaque-provider".into());
        }
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        let service = &fixture.service;

        service
            .agent_tracker
            .lock()
            .update_output("pty-stop", [5u8; 32]);
        service.agent_tracker.lock().set_idle("pty-stop");
        assert_eq!(
            service.stop_decision(&tab_id),
            StopDecision::NotWorking(TabStatus::Idle)
        );

        service
            .agent_tracker
            .lock()
            .update_output("pty-stop", [6u8; 32]);
        assert!(matches!(
            service.stop_decision(&tab_id),
            StopDecision::Interrupt { .. }
        ));

        service.mux.write().get_tab_mut(&tab_id).unwrap().agent_type = None;
        assert_eq!(service.stop_decision(&tab_id), StopDecision::NoAgent);
    }
}
