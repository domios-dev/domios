use tauri::State;

use crate::service::ServiceState;

#[derive(serde::Serialize)]
pub struct InstallTabResult {
    pub tab_id: String,
    pub launch_state: &'static str,
}

fn dispatch_install_command(
    svc: &ServiceState,
    tab_id: &str,
    install_cmd: &str,
) -> Result<(), String> {
    svc.ensure_tab_pty_for_send(tab_id)
        .map_err(|error| format!("installer PTY launch failed: {error}"))?;
    let pty_id = svc
        .resolve_pty_id(tab_id)
        .map_err(|error| format!("installer PTY acknowledgement failed: {error}"))?;
    svc.send_shell_spawn(&pty_id, install_cmd)
        .map_err(|error| format!("installer command dispatch failed: {error}"))
}

fn install_provider_in_tab_with_dispatch(
    svc: &ServiceState,
    provider_id: String,
    dispatch: impl FnOnce(&ServiceState, &str, &str) -> Result<(), String>,
) -> Result<InstallTabResult, String> {
    let platform = crate::agent::provider::current_platform_tag();
    let install_cmd = svc
        .agent_registry
        .with_provider(&provider_id, |p| p.install_command(platform))
        .ok_or_else(|| format!("Unknown provider: {}", provider_id))?
        .ok_or_else(|| {
            format!(
                "Provider '{}' has no install command for {}",
                provider_id, platform
            )
        })?;

    let ws_id = {
        let mux = svc.mux.write();
        mux.snapshot()
            .active_workspace_id
            .clone()
            .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
            .ok_or("No workspace found")?
    };

    let tab_id = svc.create_named_tab(&ws_id, &format!("Install {}", provider_id))?;
    if let Err(error) = dispatch(svc, &tab_id, &install_cmd) {
        let _ = svc.close_tab_with_reason(
            &tab_id,
            crate::multiplexer::types::TabCloseReason::SpawnRollback,
        );
        return Err(error);
    }
    Ok(InstallTabResult {
        tab_id,
        launch_state: "command_dispatched",
    })
}

/// Spawn a fresh tab and return only after its PTY accepts the provider's
/// platform install command. Any launch failure rolls the tab back.
pub fn install_provider_in_tab_core(
    svc: &ServiceState,
    provider_id: String,
) -> Result<InstallTabResult, String> {
    install_provider_in_tab_with_dispatch(svc, provider_id, dispatch_install_command)
}

#[cfg(debug_assertions)]
pub(crate) fn install_provider_in_tab_with_fixture_failure(
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
        crate::api::events::emit_everywhere(
            &svc.app_handle,
            "provider-install-fixture-dispatch-refused",
            serde_json::json!({ "tab_id": tab_id, "pty_id": pty_id }),
        );
        Err(failure)
    })
}

#[tauri::command]
pub async fn install_provider_in_tab(
    svc: State<'_, ServiceState>,
    provider_id: String,
) -> Result<InstallTabResult, String> {
    let svc = (*svc).clone();
    tokio::task::spawn_blocking(move || install_provider_in_tab_core(&svc, provider_id))
        .await
        .map_err(|error| format!("installer launch worker failed: {error}"))?
}

#[tauri::command]
pub fn complete_general_agent_first_run(
    svc: State<'_, ServiceState>,
    config: State<'_, crate::config::ConfigState>,
    app: tauri::AppHandle,
    provider_id: String,
    full_access: bool,
) -> Result<(), String> {
    crate::service::general_agent::complete_first_run_core(
        &svc,
        &config,
        &app,
        &provider_id,
        full_access,
    )
}

#[tauri::command]
pub fn restart_general_agent(
    svc: State<'_, ServiceState>,
    config: State<'_, crate::config::ConfigState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let svc = (*svc).clone();
    let mux = svc.mux.clone();
    let pty = svc.pty.clone();
    let screen_state = svc.screen_state.clone();
    let config = (*config).clone();
    std::thread::spawn(move || {
        crate::service::general_agent::restart_general_agent(
            &svc,
            &mux,
            &pty,
            &screen_state,
            &config,
            &app,
        );
    });
    Ok(())
}

#[tauri::command]
pub fn restart_general_agent_tab(
    svc: State<'_, ServiceState>,
    config: State<'_, crate::config::ConfigState>,
    app: tauri::AppHandle,
    tab_id: String,
) -> Result<(), String> {
    let svc = (*svc).clone();
    let mux = svc.mux.clone();
    let should_restart =
        crate::service::general_agent::validate_general_agent_tab_retry(&tab_id, &mux)?;
    if !should_restart {
        return Ok(());
    }
    let pty = svc.pty.clone();
    let screen_state = svc.screen_state.clone();
    let config = (*config).clone();
    std::thread::spawn(move || {
        crate::service::general_agent::restart_general_agent(
            &svc,
            &mux,
            &pty,
            &screen_state,
            &config,
            &app,
        );
    });
    Ok(())
}

#[tauri::command]
pub async fn restart_agent_tab(svc: State<'_, ServiceState>, tab_id: String) -> Result<(), String> {
    let svc = (*svc).clone();
    tokio::task::spawn_blocking(move || {
        svc.agent_restart(
            &tab_id,
            false,
            crate::service::agent_restart::DEFAULT_GRACE_SECS,
        )
    })
    .await
    .map_err(|error| format!("restart worker failed: {error}"))?
    .map(|_| ())
    .map_err(|error| match error {
        crate::service::agent_restart::AgentRestartError::NotFound(message)
        | crate::service::agent_restart::AgentRestartError::BadRequest(message)
        | crate::service::agent_restart::AgentRestartError::Internal(message) => message,
        crate::service::agent_restart::AgentRestartError::RateLimited { retry_after_secs } => {
            format!("Retry is available in {retry_after_secs} seconds")
        }
    })
}

#[tauri::command]
pub fn list_provider_update_prompts(
    svc: State<'_, ServiceState>,
) -> Vec<crate::service::provider_update::ProviderUpdatePromptView> {
    svc.list_provider_update_prompts()
}

#[tauri::command]
pub async fn respond_provider_update_prompt(
    svc: State<'_, ServiceState>,
    prompt_id: String,
    accept: bool,
) -> Result<(), String> {
    let svc = (*svc).clone();
    tokio::task::spawn_blocking(move || svc.respond_provider_update_prompt(&prompt_id, accept))
        .await
        .map_err(|error| format!("provider update worker failed: {error}"))?
}

#[tauri::command]
pub async fn acknowledge_provider_update_prompt(
    svc: State<'_, ServiceState>,
    prompt_id: String,
) -> Result<(), String> {
    let svc = (*svc).clone();
    tokio::task::spawn_blocking(move || svc.acknowledge_provider_update_prompt(&prompt_id))
        .await
        .map_err(|error| format!("provider update worker failed: {error}"))?
}

/// Probe for a CLI binary. Used by the per-provider Installation card in
/// Settings → Agents to render the install/missing badge. Checks the process
/// PATH plus platform fallback dirs because GUI apps often inherit a minimal
/// PATH. Returns `Ok(true)` when the binary is found, `Ok(false)` when missing.
#[tauri::command]
pub fn check_cli_installed(binary: String) -> Result<bool, String> {
    Ok(crate::service::spawn::cli_binary_on_path(&binary))
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
    #[cfg_attr(
        not(windows),
        ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
    )]
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
    #[cfg_attr(
        not(windows),
        ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
    )]
    fn production_transaction_rolls_back_real_provisional_tab_on_dispatch_failure() {
        let (_fixture, service) = fixture_with_provider("install-failure", "echo never-dispatched");
        let failure = "opaque installer dispatch failure b921";
        let provisional_pty = RefCell::new(None);
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
        assert!(mux
            .snapshot()
            .tabs
            .values()
            .all(|tab| tab.title != "Install install-failure"));
        drop(mux);
        let pty_id = provisional_pty
            .into_inner()
            .expect("fixture must link a provisional PTY");
        assert!(service.pty.lock().get_session(&pty_id).is_none());
    }
}

/// TTL-gated re-probe for a reader that just opened the model picker. An open
/// is an explicit user action, not a poll, and the TTL bounds it to at most one
/// probe per window — except while a verdict is still unknown, which is retried.
#[tauri::command]
pub async fn refresh_provider_models(
    app: tauri::AppHandle,
    svc: State<'_, ServiceState>,
    provider_id: String,
) -> Result<bool, String> {
    let Some(provider) = svc.agent_registry.provider_arc(&provider_id) else {
        return Ok(false);
    };
    let refreshed = crate::agent::model_catalog::refresh_for_reader(provider)
        .await
        .is_some();
    if refreshed {
        crate::api::events::emit_everywhere(
            &app,
            "models-updated",
            serde_json::json!({ "provider": provider_id }),
        );
    }
    Ok(refreshed)
}
