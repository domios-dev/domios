use crate::multiplexer::restore_policy::{restore_on_boot, RestoreDecision};
use crate::service::ServiceContext;

pub fn initialize(svc: &ServiceContext, active_window_hours: u64) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64;
    let window_ms = active_window_hours.saturating_mul(3_600_000);
    let mut mux = svc.mux.write();
    let general_workspace = super::general_agent::general_workspace_id(&mux);
    let snapshot = mux.snapshot();
    for workspace in &snapshot.workspaces {
        for id in &workspace.tabs {
            if let Some(tab) = mux.get_tab_mut(id) {
                tab.frozen = tab.restored_from_snapshot
                    && !tab.protected
                    && restore_on_boot(
                        general_workspace.as_deref() == Some(workspace.id.as_str()),
                        tab.protected,
                        tab.last_active_at_ms,
                        now_ms,
                        window_ms,
                    ) == RestoreDecision::Freeze;
            }
        }
    }
}

impl ServiceContext {
    pub fn wake_frozen_tab(&self, tab_id: &str) -> Result<(), String> {
        wake_with(self, tab_id, |id| {
            if self.tab_chat_backing(id).is_some() {
                self.chatbackend_reopen_if_restored(id).map(|_| ())
            } else {
                self.ensure_tab_pty_for_send(id)
            }
        })
    }
}

fn wake_with(
    svc: &ServiceContext,
    tab_id: &str,
    materialize: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    let id = {
        let mux = svc.mux.read();
        let id = mux
            .resolve_tab_id(tab_id)
            .ok_or_else(|| format!("Tab {tab_id} not found"))?;
        if !mux.get_tab(&id).is_some_and(|tab| tab.frozen) {
            return Ok(());
        }
        id
    };
    {
        let mut mux = svc.mux.write();
        let tab = mux.get_tab_mut(&id).ok_or("Sleeping tab closed")?;
        tab.frozen = false;
        tab.record_activity();
    }
    let result = materialize(&id);
    svc.emit_state_changed();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;

    #[test]
    fn startup_keeps_general_awake_and_freezes_unknown_project_activity() {
        let (mut mux, general) = MultiplexerState::new();
        mux.register_alias(super::super::general_agent::GENERAL_ALIAS, &general);
        let general_ws = mux.list_workspaces()[0].id.clone();
        let sibling = mux.create_tab(&general_ws).unwrap();
        for id in [&general, &sibling] {
            mux.get_tab_mut(id).unwrap().restored_from_snapshot = true;
        }
        let fixture = ServiceFixture::new(mux);
        initialize(&fixture.service, 48);
        for id in [&general, &sibling] {
            assert!(!fixture.service.mux.read().get_tab(id).unwrap().frozen);
        }
    }

    #[test]
    fn frozen_address_materializes_before_delivery() {
        let (mut mux, id) = MultiplexerState::new();
        mux.get_tab_mut(&id).unwrap().frozen = true;
        let fixture = ServiceFixture::new(mux);
        let mut events = Vec::new();
        wake_with(&fixture.service, &id, |resolved| {
            assert_eq!(resolved, id);
            assert!(!fixture.service.mux.read().get_tab(resolved).unwrap().frozen);
            events.push("materialize");
            Ok(())
        })
        .unwrap();
        events.push("deliver");
        assert_eq!(events, ["materialize", "deliver"]);
    }
}
