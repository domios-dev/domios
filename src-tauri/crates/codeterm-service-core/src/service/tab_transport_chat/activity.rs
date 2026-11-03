use super::liveness::BoundSession;
use super::*;
use crate::multiplexer::types::TabBacking;
use codeterm_types::pty::{TabVerdict, TimelineState};

impl ServiceContext {
    pub(super) fn observe_chatbackend_activity(
        &self,
        plugin: &str,
        session: &str,
        bound: &BoundSession,
        delta: &StructuredChat,
    ) {
        let Some(done) = delta.done else {
            return;
        };
        let mut mux = self.mux.write();
        let Some(tab) = mux.get_tab_mut(&bound.tab_id).filter(|tab| {
            tab.session_binding_nonce.as_deref() == Some(bound.binding_nonce.as_str())
                && matches!(&tab.backing, TabBacking::ChatBackend { plugin_id, session_id }
                    if plugin_id == plugin && session_id == session)
        }) else {
            return;
        };
        if bound.current().is_err() {
            return;
        }
        let state = if done {
            TimelineState::Idle
        } else {
            TimelineState::Working
        };
        if tab
            .agent_state
            .as_ref()
            .is_some_and(|live| live.state == state)
        {
            return;
        }
        tab.agent_state = Some(
            TabVerdict {
                state,
                subtype: None,
            }
            .into(),
        );
        drop(mux);
        self.emit_state_changed(super::super::MuxEmitCause::RuntimeSignal);
    }
}
