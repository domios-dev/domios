use crate::multiplexer::types::{Tab, TabBacking};
use crate::service::ServiceContext;
use codeterm_pty::multiplexer::restore_policy::{restore_on_boot, RestoreDecision};
use codeterm_types::pty::BootStage;
use std::collections::HashSet;
use std::sync::LazyLock;

static WAKING: LazyLock<parking_lot::Mutex<HashSet<String>>> = LazyLock::new(Default::default);

struct WakeRequest(String);

impl Drop for WakeRequest {
    fn drop(&mut self) {
        WAKING.lock().remove(&self.0);
    }
}

fn wake_eligible(tab: &Tab, explicit: bool) -> bool {
    tab.frozen
        || explicit
            && !tab.protected
            && (tab.boot_stage == Some(BootStage::SpawnFailed)
                || matches!(tab.backing, TabBacking::Pty)
                    && tab.pty_id.is_none()
                    && !tab
                        .close_restore
                        .as_ref()
                        .is_some_and(|closed| closed.stopped))
}

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
        self.wake_tab(tab_id, false)
    }

    pub fn wake_tab_on_request(&self, tab_id: &str) -> Result<(), String> {
        self.wake_tab(tab_id, true)
    }

    fn wake_tab(&self, tab_id: &str, allow_failed: bool) -> Result<(), String> {
        wake_with(self, tab_id, allow_failed, |id, retry_started_agent| {
            if retry_started_agent {
                return self
                    .agent_restart(id, false, super::agent_restart::DEFAULT_GRACE_SECS)
                    .map(|_| ())
                    .map_err(|reason| match reason {
                        super::agent_restart::AgentRestartError::NotFound(reason)
                        | super::agent_restart::AgentRestartError::BadRequest(reason)
                        | super::agent_restart::AgentRestartError::Internal(reason) => reason,
                        other => format!("{other:?}"),
                    });
            }
            if self.tab_chat_backing(id).is_some() {
                self.chatbackend_reopen_if_restored(id).map(|_| ())
            } else {
                let cwd = self.mux.read().get_tab(id).and_then(|tab| tab.cwd.clone());
                self.validate_request_cwd(None, Some(id), cwd.as_deref())
                    .map_err(|error| error.data().to_string())?;
                self.ensure_tab_pty_for_send(id)
            }
        })
    }
}

fn wake_with(
    svc: &ServiceContext,
    tab_id: &str,
    allow_failed: bool,
    materialize: impl FnOnce(&str, bool) -> Result<(), String>,
) -> Result<(), String> {
    let id = {
        let mux = svc.mux.read();
        let id = mux
            .resolve_tab_id(tab_id)
            .ok_or_else(|| format!("Tab {tab_id} not found"))?;
        if !mux
            .get_tab(&id)
            .is_some_and(|tab| wake_eligible(tab, allow_failed))
        {
            return Ok(());
        }
        id
    };
    if !WAKING.lock().insert(id.clone()) {
        return Ok(());
    }
    let _request = WakeRequest(id.clone());
    let retry_started_agent = {
        let mut mux = svc.mux.write();
        let tab = mux.get_tab_mut(&id).ok_or("Sleeping tab closed")?;
        if !wake_eligible(tab, allow_failed) {
            return Ok(());
        }
        let retry_started_agent = !tab.frozen
            && tab.boot_stage == Some(BootStage::SpawnFailed)
            && !tab.restored_from_snapshot
            && tab.agent_type.is_some();
        tab.frozen = false;
        tab.record_activity();
        let stage = if matches!(
            tab.backing,
            crate::multiplexer::types::TabBacking::ChatBackend { .. }
        ) {
            BootStage::AgentLaunching
        } else {
            BootStage::PtyStarting
        };
        super::boot_stage::apply_boot_stage(tab, stage, None);
        retry_started_agent
    };
    svc.emit_state_changed();
    let result = materialize(&id, retry_started_agent);
    if let Err(reason) = &result {
        if let Some(tab) = svc.mux.write().get_tab_mut(&id) {
            tab.agent_booting = false;
            super::boot_stage::apply_boot_stage(tab, BootStage::SpawnFailed, Some(reason));
        }
    }
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
    fn materializer_failure_is_visible_and_explicit_wake_retries_once() {
        let (mut mux, id) = MultiplexerState::new();
        mux.get_tab_mut(&id).unwrap().frozen = true;
        mux.get_tab_mut(&id).unwrap().agent_type = Some("fixture".into());
        let fixture = ServiceFixture::new(mux);
        let reason = "fixture materializer error";
        assert_eq!(
            wake_with(&fixture.service, &id, false, |_, _| Err(reason.into())),
            Err(reason.into())
        );
        {
            let mux = fixture.service.mux.read();
            let tab = mux.get_tab(&id).unwrap();
            assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
            assert_eq!(tab.boot_failure_reason.as_deref(), Some(reason));
        }
        wake_with(&fixture.service, &id, false, |_, _| {
            panic!("address retried failed wake")
        })
        .unwrap();
        let calls = std::cell::Cell::new(0);
        wake_with(
            &fixture.service,
            &id,
            true,
            |resolved, retry_started_agent| {
                assert!(retry_started_agent);
                calls.set(calls.get() + 1);
                let tab = fixture
                    .service
                    .mux
                    .read()
                    .get_tab(resolved)
                    .cloned()
                    .unwrap();
                assert_eq!(tab.boot_stage, Some(BootStage::PtyStarting));
                assert!(tab.boot_failure_reason.is_none());
                wake_with(&fixture.service, resolved, true, |_, _| {
                    panic!("wake duplicated while materializing")
                })?;
                fixture
                    .service
                    .mux
                    .write()
                    .get_tab_mut(resolved)
                    .unwrap()
                    .pty_id = Some("materialized".into());
                Ok(())
            },
        )
        .unwrap();
        wake_with(&fixture.service, &id, true, |_, _| {
            panic!("completed wake duplicated")
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn repeated_wakes_materialize_once() {
        let (mut mux, id) = MultiplexerState::new();
        mux.get_tab_mut(&id).unwrap().frozen = true;
        let fixture = ServiceFixture::new(mux);
        let calls = std::cell::Cell::new(0);
        for _ in 0..2 {
            wake_with(&fixture.service, &id, false, |_, _| {
                calls.set(calls.get() + 1);
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn missing_backend_wake_eligibility_excludes_owned_stopped_and_protected_tabs() {
        let (mut mux, id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.protected = false;
        tab.frozen = false;
        tab.pty_id = None;
        assert!(!wake_eligible(tab, false));
        assert!(wake_eligible(tab, true));
        tab.protected = true;
        assert!(!wake_eligible(tab, true));
        tab.protected = false;
        tab.pty_id = Some("owned".into());
        assert!(!wake_eligible(tab, true));
        tab.pty_id = None;
        tab.close_restore = Some(crate::multiplexer::types::CloseRestoreTabState {
            stopped: true,
            ..Default::default()
        });
        assert!(!wake_eligible(tab, true));
    }

    #[test]
    fn awake_missing_backend_materializes_without_restart_and_publishes_failure() {
        let (mut mux, id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.protected = false;
        tab.frozen = false;
        tab.pty_id = None;
        tab.agent_session_id = Some("exact-session".into());
        let fixture = ServiceFixture::new(mux);
        let reason = r#"{"code":"workspace_cwd_missing","path":"/deleted"}"#;
        assert_eq!(
            wake_with(&fixture.service, &id, true, |resolved, restart| {
                assert_eq!(resolved, id);
                assert!(!restart);
                wake_with(&fixture.service, resolved, true, |_, _| {
                    panic!("duplicate wake")
                })?;
                Err(reason.into())
            }),
            Err(reason.into())
        );
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        assert_eq!(tab.agent_session_id.as_deref(), Some("exact-session"));
        assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
        assert_eq!(tab.boot_failure_reason.as_deref(), Some(reason));
    }

    #[test]
    fn frozen_address_materializes_before_delivery() {
        let (mut mux, id) = MultiplexerState::new();
        mux.get_tab_mut(&id).unwrap().frozen = true;
        let fixture = ServiceFixture::new(mux);
        let mut events = Vec::new();
        wake_with(&fixture.service, &id, false, |resolved, _| {
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
