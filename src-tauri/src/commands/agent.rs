use tauri::State;

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
    caller_tab_id: Option<String>,
) -> Result<InstallTabResult, String> {
    let svc = (*svc).clone();
    tokio::task::spawn_blocking(move || {
        crate::service::provider_cli_actions::launch(
            &svc,
            &provider_id,
            crate::service::provider_cli_actions::CliOperation::Install,
            caller_tab_id,
        )
    })
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
pub async fn spawn_general_agent_session(
    svc: State<'_, ServiceState>,
    config: State<'_, crate::config::ConfigState>,
    params: crate::service::general_agent::FreshGeneralSessionRequest,
) -> Result<crate::service::transport::AtomicSpawn, String> {
    let svc = (*svc).clone();
    let config = (*config).clone();
    tokio::task::spawn_blocking(move || svc.spawn_fresh_general_session(&config, params))
        .await
        .map_err(|error| format!("General session launch worker failed: {error}"))?
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
pub async fn restart_agent_tab(
    svc: State<'_, ServiceState>,
    tab_id: String,
    fresh: Option<bool>,
) -> Result<(), String> {
    let svc = (*svc).clone();
    tokio::task::spawn_blocking(move || {
        svc.agent_restart_observed_with_options(
            &tab_id,
            crate::service::agent_restart::RestartOptions {
                grace_secs: crate::service::agent_restart::DEFAULT_GRACE_SECS,
                fresh: fresh.unwrap_or(false),
                ..Default::default()
            },
        )
        .map_err(|error| match error {
            crate::service::agent_restart::RestartAttemptError::Request(error) => error,
            crate::service::agent_restart::RestartAttemptError::Failed { reason, .. } => {
                crate::service::agent_restart::AgentRestartError::Internal(reason)
            }
        })
    })
    .await
    .map_err(|error| format!("restart worker failed: {error}"))?
    .map(|_| ())
    .map_err(|error| match error {
        crate::service::agent_restart::AgentRestartError::BackgroundWork { items } => format!(
            "restart refused while background work is running: {}",
            items.join(", ")
        ),
        crate::service::agent_restart::AgentRestartError::NotFound(message)
        | crate::service::agent_restart::AgentRestartError::BadRequest(message)
        | crate::service::agent_restart::AgentRestartError::Internal(message) => message,
        crate::service::agent_restart::AgentRestartError::RateLimited { retry_after_secs } => {
            format!("Retry is available in {retry_after_secs} seconds")
        }
    })
}

#[tauri::command]
pub async fn fork_agent_tab(
    svc: State<'_, ServiceState>,
    tab_id: String,
) -> Result<crate::service::agent_fork::ForkResult, String> {
    let svc = svc.inner().clone();
    tokio::task::spawn_blocking(move || svc.agent_fork(&tab_id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn switch_agent_model(
    svc: State<'_, ServiceState>,
    tab_id: String,
    model: String,
    reasoning_effort: Option<String>,
    runtime_mode: Option<crate::service::codex_rpc::model::RuntimeMode>,
    restart_resume: Option<bool>,
    cancel: Option<bool>,
) -> Result<serde_json::Value, String> {
    let svc = (*svc).clone();
    if cancel == Some(true) {
        svc.cancel_staged_model_switch(&tab_id);
        return Ok(serde_json::json!({"ok": true, "cancelled": true}));
    }
    tokio::task::spawn_blocking(move || {
        svc.agent_switch_model_with_restart_fallback(
            &tab_id,
            &model,
            reasoning_effort.as_deref(),
            runtime_mode,
            crate::service::codex_rpc::model::ModelSwitchOptions {
                force: false,
                restart_resume: restart_resume == Some(true),
            },
        )
    })
    .await
    .map_err(|error| format!("model switch worker failed: {error}"))?
    .map(|outcome| crate::api::agent_model_routes::model_switch_success_body(&outcome))
    .map_err(|error| error.to_string())
}

/// Re-probe one CLI binary through the shared resolver, dropping its memo first
/// so an explicit user re-check sees an install that landed after startup. Walks
/// the filesystem, so it runs off the async runtime.
#[tauri::command]
pub async fn check_cli_installed(binary: String) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || crate::service::spawn::cli_binary_on_path(&binary))
        .await
        .map_err(|error| format!("cli probe worker failed: {error}"))
}

#[tauri::command]
pub async fn provider_cli_status(
    svc: State<'_, ServiceState>,
    provider_id: String,
) -> Result<codeterm_plugin::provider_cli::ProviderCliStatus, String> {
    let svc = (*svc).clone();
    tokio::task::spawn_blocking(move || {
        crate::service::provider_cli::provider_cli_status(
            &svc.provider_registry,
            &provider_id,
            crate::agent::provider::current_platform_tag(),
        )
    })
    .await
    .map_err(|error| format!("CLI status worker failed: {error}"))?
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

/// Explicitly refresh a provider's catalogue for a caller that requests it.
/// The live refresh runs on the blocking pool or the chatBackend catalogue thread.
#[tauri::command]
pub async fn refresh_provider_models(
    svc: State<'_, ServiceState>,
    provider_id: String,
) -> Result<bool, String> {
    if let Some(provider) = svc.agent_registry.provider_arc(&provider_id) {
        crate::agent::model_catalog::invalidate(&provider_id);
        return Ok(crate::agent::model_catalog::refresh_models(provider, true)
            .await
            .is_some());
    }
    if svc.chatbackend_registry.get(&provider_id).is_none() {
        return Ok(false);
    }
    crate::agent::model_catalog::invalidate(&provider_id);
    let registry = svc.chatbackend_registry.clone();
    tokio::task::spawn_blocking(move || registry.refresh_and_wait(&provider_id))
        .await
        .map_err(|error| format!("chatBackend catalogue refresh failed: {error}"))
}

/// Per-group usage for the model picker. The provider owns both the grouping
/// and the source; this only carries its answer, so a new provider needs no
/// core edit. The call may read a provider-owned store, so it stays off the
/// async runtime. An unknown provider answers with no groups, never an error.
#[tauri::command]
pub async fn provider_model_usage(
    svc: State<'_, ServiceState>,
    provider_id: String,
) -> Result<Vec<crate::agent::provider::ModelGroupUsage>, String> {
    let Some(provider) = svc.agent_registry.provider_arc(&provider_id) else {
        return Ok(Vec::new());
    };
    tokio::task::spawn_blocking(move || provider.model_group_usage())
        .await
        .map_err(|error| format!("model usage task failed: {error}"))
}
