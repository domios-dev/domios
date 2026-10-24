use super::shell_exec::{ExecCompletion, ExecWaiter};
use super::spawn::SpawnTabSpec;
use super::ServiceContext;
use crate::service::MuxEmitCause;
use codeterm_plugin::provider_cli::ProviderCliStatus;
mod update;
pub use update::{job as update_job, revision as update_revision};

#[derive(Debug, Clone, Copy, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CliOperation {
    Install,
    Update,
}

impl CliOperation {
    fn label(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Update => "update",
        }
    }
}

pub(crate) fn operation_command(
    status: &ProviderCliStatus,
    operation: CliOperation,
    platform: &str,
) -> Result<String, String> {
    if !status.required {
        return Err("This provider requires no CLI".into());
    }
    let command = match operation {
        CliOperation::Install => &status.installer,
        CliOperation::Update => &status.updater,
    };
    command.clone().ok_or_else(|| {
        if matches!(operation, CliOperation::Update) {
            if let Some(reason) = &status.update_refusal {
                return reason.clone();
            }
        }
        format!(
            "Provider has no CLI {} command for {platform}",
            operation.label()
        )
    })
}

#[derive(serde::Serialize)]
pub struct CliLaunch {
    pub tab_id: String,
    pub workspace_id: String,
    pub launch_state: &'static str,
    pub warnings: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum CompletionRoute {
    AgentReport,
    SystemLog,
}

fn completion_route(agent_type: Option<&str>, agent_booting: bool) -> CompletionRoute {
    if agent_type.is_some() || agent_booting {
        CompletionRoute::AgentReport
    } else {
        CompletionRoute::SystemLog
    }
}

fn report_completion(svc: &ServiceContext, caller: &str, utility_tab: &str, summary: &str) {
    let route = {
        let mux = svc.mux.read();
        mux.get_tab(caller)
            .map(|tab| completion_route(tab.agent_type.as_deref(), tab.agent_booting))
            .unwrap_or(CompletionRoute::SystemLog)
    };
    match route {
        CompletionRoute::AgentReport => {
            if let Err(error) = super::messenger::Messenger::deliver(
                svc,
                caller,
                super::messenger::MessageEnvelope::report("ProviderCLI", utility_tab),
                summary,
            ) {
                log::warn!("[provider-cli] result delivery failed: {error}");
            }
        }
        CompletionRoute::SystemLog => {
            svc.log_system_notice(
                "Provider CLI result".into(),
                Some(summary.into()),
                Some("info"),
            );
        }
    }
}

pub fn launch(
    svc: &ServiceContext,
    provider: &str,
    operation: CliOperation,
    caller: Option<String>,
) -> Result<CliLaunch, String> {
    launch_with_dispatch(svc, provider, operation, caller, |svc, tab_id, command| {
        let pty = svc.resolve_pty_id(tab_id)?;
        svc.send_shell_spawn_after_ready(&pty, command)
    })
}

pub fn launch_with_dispatch(
    svc: &ServiceContext,
    provider: &str,
    operation: CliOperation,
    caller: Option<String>,
    dispatch: impl FnOnce(&ServiceContext, &str, &str) -> Result<(), String>,
) -> Result<CliLaunch, String> {
    let platform = crate::agent::provider::current_platform_tag();
    let mut status =
        super::provider_cli::provider_cli_status(&svc.provider_registry, provider, platform)?;
    let updater_provider = svc.agent_registry.provider_arc(provider);
    let capability = updater_provider
        .as_ref()
        .and_then(|p| p.provider_self_update_capability());
    let explicit_update = matches!(operation, CliOperation::Update) && capability.is_some();
    let permit = if explicit_update {
        Some(update::reserve(provider)?)
    } else {
        None
    };
    if let Some(capability) = capability.filter(|_| explicit_update) {
        let binary = status.binary.as_deref().ok_or("CLI binary is unknown")?;
        let path = match codeterm_plugin::js_runtime::probe_bin(binary).resolution {
            codeterm_plugin::js_runtime::BinResolution::Found(path) => path,
            _ => return Err("CLI executable cannot be resolved".into()),
        };
        let installation = codeterm_plugin::provider_self_update::detect_installation(
            &capability,
            &path,
            platform,
        )
        .ok_or(
            "Installation method is unknown or unsupported; update using its original installer",
        )?;
        status.updater = codeterm_plugin::provider_self_update::update_command(
            &capability,
            &path,
            &installation,
            platform,
        );
        status.resolved_path = Some(path);
    }
    let mut command = operation_command(&status, operation, platform)?;
    let mut target = None;
    if matches!(operation, CliOperation::Update)
        && (explicit_update || command.contains("{version}"))
    {
        let updater_provider = svc
            .agent_registry
            .provider_arc(provider)
            .ok_or_else(|| "CLI update provider is unavailable".to_string())?;
        let available = super::provider_update::newer_target(updater_provider.check_for_update())
            .ok_or_else(|| "No supported CLI update target is available".to_string())?;
        command = if command.contains("{version}") {
            super::provider_update::compatible_command_for_target(
                &command,
                &available.latest_version,
                updater_provider.cli_compatibility().as_ref(),
            )?
        } else {
            command
        };
        target = Some(available);
    }
    command = update::prepare_command(command, platform)?;
    let prerequisites = match operation {
        CliOperation::Install => &status.installer_prerequisites,
        CliOperation::Update => &status.updater_prerequisites,
    };
    let prerequisite_hint = codeterm_plugin::provider_cli::missing_prerequisite_hint(prerequisites);
    let handle = svc.spawn_tab(SpawnTabSpec {
        shell: None,
        timeout: explicit_update.then(|| "10m".into()),
        purpose: crate::multiplexer::types::ShellPurpose::Deploy,
        creator_tab_id: caller.clone(),
        title: Some(
            match operation {
                CliOperation::Install => "CliInstall",
                CliOperation::Update => "CliUpdate",
            }
            .into(),
        ),
        description: Some(fit_tab_description(&format!(
            "{} CLI for {provider}{}",
            operation.label(),
            prerequisite_hint
                .as_ref()
                .map(|hint| format!("; {hint}"))
                .unwrap_or_default()
        ))),
        ..Default::default()
    })?;
    let tab_id = handle.tab_id;
    let workspace_id = handle.workspace_id;
    let dispatched = (|| {
        svc.ensure_tab_pty_for_send(&tab_id)?;
        let pty = svc.resolve_pty_id(&tab_id)?;
        if !super::pty_io::await_shell_prompt(
            &svc.screen_state,
            &pty,
            std::time::Duration::from_secs(30),
        ) {
            return Err("Provider CLI utility shell did not become ready".into());
        }
        let waiter = ExecWaiter::register(&pty);
        dispatch(svc, &tab_id, &command)?;
        Ok::<_, String>(waiter)
    })();
    let waiter = match dispatched {
        Ok(waiter) => waiter,
        Err(error) => {
            let _ = svc.close_tab_with_reason(
                &tab_id,
                crate::multiplexer::types::TabCloseReason::SpawnRollback,
            );
            return Err(error);
        }
    };
    let service = svc.clone();
    let version_path = status.resolved_path.clone();
    let monitor_tab = tab_id.clone();
    let provider = provider.to_owned();
    if explicit_update {
        update::record(
            &provider,
            codeterm_plugin::provider_cli::ProviderCliUpdateJob {
                tab_id: tab_id.clone(),
                state: "running".into(),
                summary: "Updating CLI…".into(),
            },
        );
        svc.ui.emit(
            "provider-cli-started",
            serde_json::json!({ "provider": provider, "tab_id": tab_id }),
        );
    }
    crate::async_runtime::spawn(async move {
        let completion = waiter.wait(std::time::Duration::from_secs(600)).await;
        let _ = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            if completion.is_none() {
                let _ = service.close_tab_with_reason(&monitor_tab, crate::multiplexer::types::TabCloseReason::SpawnRollback);
            }
            codeterm_plugin::js_runtime::refresh_user_path();
            crate::agent::model_catalog::invalidate(&provider);
            let old_version = target.as_ref().map(|v| v.current_version.clone());
            let new_version = if explicit_update { version_path.as_deref().and_then(|path| codeterm_plugin::provider_version::refresh(path).version) } else { None };
            let status = super::provider_cli::provider_cli_status(&service.provider_registry, &provider, platform).ok();
            let success = if explicit_update {
                update::verified(completion.and_then(ExecCompletion::exit_code), target.as_ref().map(|v| v.latest_version.as_str()), new_version.as_deref())
            } else { completion.and_then(ExecCompletion::exit_code) == Some(0) && status.as_ref().and_then(|s| s.installed) == Some(true) };
            let summary = if explicit_update {
                if success { format!("{} CLI updated to {}", provider, new_version.as_deref().unwrap_or_default()) }
                else { format!("{}; installed version could not be verified as the requested target", completion_summary(&provider, operation, completion, status.as_ref())) }
            } else { completion_summary(&provider, operation, completion, status.as_ref()) };
            if explicit_update {
                update::record(&provider, codeterm_plugin::provider_cli::ProviderCliUpdateJob {
                    tab_id: monitor_tab.clone(), state: if success { "succeeded" } else { "failed" }.into(), summary: summary.clone(),
                });
                super::provider_update::publish_available(&service, &provider, if success { None } else { target });
            }
            let _ = service
                .mux
                .write()
                .set_tab_description(&monitor_tab, Some(fit_tab_description(&summary)));
            service.emit_state_changed(MuxEmitCause::TabLifecycle);
            if success {
                if let (Some(old), Some(new)) = (old_version.as_deref(), new_version.as_deref()) {
                    super::provider_runtime_update::completed(&service, &provider, old, new);
                }
            }
            if matches!(operation, CliOperation::Install) {
                publish_availability(&service, &provider, &monitor_tab);
            }
            service.ui.emit("provider-cli-completed", serde_json::to_value(serde_json::json!({
                "provider": provider, "operation": operation, "tab_id": monitor_tab,
                "success": success, "old_version": old_version, "new_version": new_version,
                "exit_code": completion.and_then(ExecCompletion::exit_code), "status": status, "summary": summary,
            })).unwrap_or_default());
            if let Some(caller) = caller.filter(|_| !explicit_update) {
                report_completion(&service, &caller, &monitor_tab, &summary);
            }
        }).await;
    });
    Ok(CliLaunch {
        tab_id,
        workspace_id,
        launch_state: "command_dispatched",
        warnings: prerequisite_hint.into_iter().collect(),
    })
}

/// Re-probe the provider the installer just finished and push its answer, so
/// an open picker flips without polling.
fn publish_availability(svc: &ServiceContext, provider: &str, tab_id: &str) {
    if let Some(agent) = svc.agent_registry.provider_arc(provider) {
        crate::agent::model_binary_watch::on_catalogue_input_changed(agent);
    }
    let availability = svc
        .provider_registry
        .describe_all()
        .into_iter()
        .find(|descriptor| descriptor.id == provider)
        .map(|descriptor| descriptor.availability)
        .unwrap_or_default();
    svc.ui.emit(
        "provider-availability-changed",
        serde_json::json!({ "provider": provider, "availability": availability, "tab_id": tab_id }),
    );
}

fn completion_summary(
    provider: &str,
    operation: CliOperation,
    completion: Option<ExecCompletion>,
    status: Option<&ProviderCliStatus>,
) -> String {
    let outcome = match completion.and_then(ExecCompletion::exit_code) {
        Some(0) => "command exited 0".to_owned(),
        Some(code) => format!("command failed with exit code {code}"),
        None if completion.is_none() => {
            "installation timed out after 10 minutes; retry the operation".into()
        }
        None => "command completion observed without an exit code".into(),
    };
    let presence = match status.and_then(|s| s.installed) {
        Some(true) => "CLI installed",
        Some(false) => "CLI still missing",
        None => "CLI availability unknown",
    };
    let mut summary = format!(
        "{provider} CLI {}: {outcome}; {presence}",
        operation.label()
    );
    if let Some(status) = status {
        let prerequisites = match operation {
            CliOperation::Install => &status.installer_prerequisites,
            CliOperation::Update => &status.updater_prerequisites,
        };
        if let Some(hint) = codeterm_plugin::provider_cli::missing_prerequisite_hint(prerequisites)
        {
            summary.push_str(&format!("; {hint}"));
        }
    }
    summary
}

// The full guidance stays in the launch warnings and the completion report.
fn fit_tab_description(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let limit = super::MAX_TAB_DESCRIPTION_CHARS;
    if flat.chars().count() <= limit {
        return flat;
    }
    let mut fitted: String = flat.chars().take(limit - 1).collect();
    fitted.push('…');
    fitted
}

#[cfg(test)]
mod tests;
