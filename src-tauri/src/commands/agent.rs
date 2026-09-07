use tauri::State;

use crate::service::ServiceState;

#[derive(serde::Serialize)]
pub struct InstallTabResult {
    pub tab_id: String,
    pub launch_state: &'static str,
}

fn complete_install_launch(
    tab_id: String,
    launch: impl FnOnce(&str) -> Result<(), String>,
    rollback: impl FnOnce(&str),
) -> Result<InstallTabResult, String> {
    if let Err(error) = launch(&tab_id) {
        rollback(&tab_id);
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
    complete_install_launch(
        tab_id,
        |tab_id| {
            svc.ensure_tab_pty_for_send(tab_id)
                .map_err(|error| format!("installer PTY launch failed: {error}"))?;
            let pty_id = svc
                .resolve_pty_id(tab_id)
                .map_err(|error| format!("installer PTY acknowledgement failed: {error}"))?;
            svc.send_shell_spawn(&pty_id, &install_cmd)
                .map_err(|error| format!("installer command dispatch failed: {error}"))
        },
        |tab_id| {
            let _ = svc.close_tab_with_reason(
                tab_id,
                crate::multiplexer::types::TabCloseReason::SpawnRollback,
            );
        },
    )
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
    use std::cell::RefCell;

    #[test]
    fn install_success_is_returned_only_after_command_dispatch_ack() {
        let events = RefCell::new(Vec::new());
        let result = complete_install_launch(
            "tab-fixture".into(),
            |tab_id| {
                events.borrow_mut().push(format!("dispatch:{tab_id}"));
                Ok(())
            },
            |_| events.borrow_mut().push("rollback".into()),
        )
        .unwrap();
        assert_eq!(events.into_inner(), vec!["dispatch:tab-fixture"]);
        assert_eq!(result.launch_state, "command_dispatched");
    }

    #[test]
    fn install_dispatch_failure_is_exact_and_rolls_back_empty_tab() {
        let events = RefCell::new(Vec::new());
        let failure = "opaque installer dispatch failure b921";
        let result = complete_install_launch(
            "tab-fixture".into(),
            |_| Err(failure.into()),
            |tab_id| events.borrow_mut().push(format!("rollback:{tab_id}")),
        );
        assert_eq!(result.err().as_deref(), Some(failure));
        assert_eq!(events.into_inner(), vec!["rollback:tab-fixture"]);
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
