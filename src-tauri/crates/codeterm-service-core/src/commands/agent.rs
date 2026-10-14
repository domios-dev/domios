use crate::service::ServiceState;

pub use crate::service::provider_cli_actions::CliLaunch as InstallTabResult;

#[cfg(any(test, debug_assertions))]
fn install_provider_in_tab_with_dispatch(
    svc: &ServiceState,
    provider_id: String,
    dispatch: impl FnOnce(&ServiceState, &str, &str) -> Result<(), String>,
) -> Result<InstallTabResult, String> {
    crate::service::provider_cli_actions::launch_with_dispatch(
        svc,
        &provider_id,
        crate::service::provider_cli_actions::CliOperation::Install,
        None,
        |svc, tab_id, command| {
            let service = std::sync::Arc::new(svc.clone());
            dispatch(&service, tab_id, command)
        },
    )
}

/// Spawn a fresh tab and return only after its PTY accepts the provider's
/// platform install command. Any launch failure rolls the tab back.
pub fn install_provider_in_tab_core(
    svc: &ServiceState,
    provider_id: String,
) -> Result<InstallTabResult, String> {
    crate::service::provider_cli_actions::launch(
        svc,
        &provider_id,
        crate::service::provider_cli_actions::CliOperation::Install,
        None,
    )
}

#[cfg(debug_assertions)]
pub fn install_provider_in_tab_with_fixture_failure(
    svc: &ServiceState,
    provider_id: String,
    failure: String,
) -> Result<InstallTabResult, String> {
    install_provider_in_tab_with_dispatch(svc, provider_id, |svc, tab_id, _| {
        svc.ensure_tab_pty_for_send(tab_id)
            .map_err(|error| format!("installer PTY launch failed: {error}"))?;
        let pty_id = svc
            .resolve_pty_id(tab_id)
            .map_err(|error| format!("installer PTY acknowledgement failed: {error}"))?;
        codeterm_events::emit_everywhere(
            &svc.app_handle,
            "provider-install-fixture-dispatch-refused",
            serde_json::json!({ "tab_id": tab_id, "pty_id": pty_id }),
        );
        Err(failure)
    })
}

#[cfg(test)]
mod install_provider_tests {
    use super::*;
    use crate::agent::provider::{
        AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery,
    };
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    use crate::session::SessionStore;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    use tauri::Manager;

    struct InstallerProvider {
        id: String,
        command: String,
    }

    impl AgentProvider for InstallerProvider {
        fn id(&self) -> &str {
            &self.id
        }
        fn display_name(&self) -> &str {
            &self.id
        }
        fn build_launch_command(&self, _params: &AgentLaunchParams) -> String {
            self.id.clone()
        }
        fn readiness_strategy(&self) -> ReadinessStrategy {
            ReadinessStrategy::PollShellPrompt {
                timeout: Duration::from_secs(10),
            }
        }
        fn task_delivery(&self) -> TaskDelivery {
            TaskDelivery::InlineFlag
        }
        fn detect_from_title(&self, _title: &str) -> bool {
            false
        }
        fn detect_from_output(&self, _text: &str) -> bool {
            false
        }
        fn session_file_path(&self, _cwd: &str, _session_id: &str) -> Option<PathBuf> {
            None
        }
        fn build_resume_command(
            &self,
            _session_id: &str,
            _skip_permissions: bool,
        ) -> Option<String> {
            None
        }
        fn detect_session_id(
            &self,
            _cwd: &str,
            _exclude: &[String],
            _max_age: Option<Duration>,
        ) -> Option<String> {
            None
        }
        fn install_command(&self, _platform: &str) -> Option<String> {
            Some(self.command.clone())
        }
    }

    fn fixture_with_provider(id: &str, command: &str) -> (ServiceFixture, ServiceState) {
        let (mut mux, _) = MultiplexerState::new();
        let workspace = mux.create_workspace("Installer fixture".into(), None).id;
        mux.set_active_workspace(&workspace).unwrap();
        let fixture = ServiceFixture::new(mux);
        let session: crate::session::SessionState = Arc::new(parking_lot::Mutex::new(
            SessionStore::for_test_dir(fixture.temp.path().join("sessions")),
        ));
        fixture._app.manage(session);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(InstallerProvider {
                id: id.into(),
                command: command.into(),
            }));
        let service = Arc::new(fixture.service.clone());
        (fixture, service)
    }

    #[test]
    fn production_core_returns_only_after_real_pty_dispatch_ack() {
        let (_fixture, service) =
            fixture_with_provider("install-success", "echo provider-install-fixture");
        let result = install_provider_in_tab_core(&service, "install-success".into()).unwrap();
        assert_eq!(result.launch_state, "command_dispatched");
        let tab = service.mux.read().get_tab(&result.tab_id).cloned().unwrap();
        let pty_id = tab
            .pty_id
            .expect("production install core must link a PTY before success");
        assert!(service.pty.lock().get_session(&pty_id).is_some());
    }

    #[test]
    fn production_transaction_rolls_back_real_provisional_tab_on_dispatch_failure() {
        let (_fixture, service) = fixture_with_provider("install-failure", "echo never-dispatched");
        let failure = "opaque installer dispatch failure b921";
        let provisional_pty = RefCell::new(None);
        let before = service.mux.read().snapshot().tabs.len();
        let result = install_provider_in_tab_with_dispatch(
            &service,
            "install-failure".into(),
            |svc, tab_id, _| {
                svc.ensure_tab_pty_for_send(tab_id).unwrap();
                provisional_pty.replace(Some(svc.resolve_pty_id(tab_id).unwrap()));
                Err(failure.into())
            },
        );
        assert_eq!(result.err().as_deref(), Some(failure));
        let mux = service.mux.read();
        assert_eq!(mux.snapshot().tabs.len(), before);
        drop(mux);
        let pty_id = provisional_pty
            .into_inner()
            .expect("fixture must link a provisional PTY");
        assert!(service.pty.lock().get_session(&pty_id).is_none());
    }
}
