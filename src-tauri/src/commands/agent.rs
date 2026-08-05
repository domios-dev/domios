use tauri::State;

use crate::service::ServiceState;

/// Result of `install_provider_in_tab` — the new tab where the install
/// command was queued. Frontend uses this to switch focus.
#[derive(serde::Serialize)]
pub struct InstallTabResult {
    pub tab_id: String,
}

/// Spawn a fresh tab in the active workspace and queue the provider's
/// install command into it. The install command comes from the provider
/// plugin's `install_command(platform)`. The Install card on the
/// per-provider settings screen calls this.
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

    let svc_clone = svc.clone();
    let tab_id_clone = tab_id.clone();
    std::thread::spawn(move || {
        let frontend_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let pty_id = loop {
            if let Some(pid) = svc_clone
                .mux
                .try_write()
                .and_then(|m| m.get_tab(&tab_id_clone).and_then(|p| p.pty_id.clone()))
            {
                break pid;
            }
            if std::time::Instant::now() > frontend_deadline {
                log::warn!(
                    "install_provider_in_tab: PTY didn't appear for Tab {}",
                    tab_id_clone
                );
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        };
        crate::service::pty_io::poll_shell_prompt(
            &svc_clone.screen_state,
            &pty_id,
            std::time::Duration::from_secs(10),
        );
        if let Err(e) = svc_clone.send_shell_spawn(&pty_id, &install_cmd) {
            log::error!("install_provider_in_tab: failed to send cmd: {}", e);
        }
    });

    Ok(InstallTabResult { tab_id })
}

#[tauri::command]
pub fn install_provider_in_tab(
    svc: State<'_, ServiceState>,
    provider_id: String,
) -> Result<InstallTabResult, String> {
    install_provider_in_tab_core(&svc, provider_id)
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
