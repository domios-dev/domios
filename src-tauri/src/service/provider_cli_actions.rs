use super::shell_exec::{ExecCompletion, ExecWaiter};
use super::spawn::SpawnTabSpec;
use super::ServiceContext;
use codeterm_plugin::provider_cli::ProviderCliStatus;

#[derive(Debug, Clone, Copy, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CliOperation {
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
        format!(
            "Provider has no CLI {} command for {platform}",
            operation.label()
        )
    })
}

#[derive(serde::Serialize)]
pub struct CliLaunch {
    pub tab_id: String,
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

pub(crate) fn launch(
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

pub(crate) fn launch_with_dispatch(
    svc: &ServiceContext,
    provider: &str,
    operation: CliOperation,
    caller: Option<String>,
    dispatch: impl FnOnce(&ServiceContext, &str, &str) -> Result<(), String>,
) -> Result<CliLaunch, String> {
    let platform = crate::agent::provider::current_platform_tag();
    let status =
        super::provider_cli::provider_cli_status(&svc.provider_registry, provider, platform)?;
    let mut command = operation_command(&status, operation, platform)?;
    if matches!(operation, CliOperation::Update) && command.contains("{version}") {
        let updater_provider = svc
            .agent_registry
            .provider_arc(provider)
            .ok_or_else(|| "CLI update provider is unavailable".to_string())?;
        let target = updater_provider
            .check_for_update()
            .ok_or_else(|| "No supported CLI update target is available".to_string())?;
        command = super::provider_update::compatible_command_for_target(
            &command,
            &target.latest_version,
            updater_provider.cli_compatibility().as_ref(),
        )?;
    }
    let prerequisites = match operation {
        CliOperation::Install => &status.installer_prerequisites,
        CliOperation::Update => &status.updater_prerequisites,
    };
    let prerequisite_hint = codeterm_plugin::provider_cli::missing_prerequisite_hint(prerequisites);
    let handle = svc.spawn_tab(SpawnTabSpec {
        shell: None,
        creator_tab_id: caller.clone(),
        title: Some(
            match operation {
                CliOperation::Install => "CliInstall",
                CliOperation::Update => "CliUpdate",
            }
            .into(),
        ),
        description: Some(format!(
            "{} CLI for {provider}{}",
            operation.label(),
            prerequisite_hint
                .as_ref()
                .map(|hint| format!("; {hint}"))
                .unwrap_or_default()
        )),
        ..Default::default()
    })?;
    let tab_id = handle.tab_id;
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
    let monitor_tab = tab_id.clone();
    let provider = provider.to_owned();
    tauri::async_runtime::spawn(async move {
        let completion = waiter.wait(std::time::Duration::from_secs(1800)).await;
        let _ = tokio::task::spawn_blocking(move || {
            let status = super::provider_cli::provider_cli_status(&service.provider_registry, &provider, platform).ok();
            let summary = completion_summary(&provider, operation, completion, status.as_ref());
            let _ = service.mux.write().set_tab_description(&monitor_tab, Some(summary.clone()));
            service.emit_state_changed();
            crate::api::events::emit_everywhere(&service.app_handle, "provider-cli-completed", serde_json::json!({
                "provider": provider, "operation": operation, "tab_id": monitor_tab,
                "exit_code": completion.and_then(ExecCompletion::exit_code), "status": status, "summary": summary,
            }));
            if let Some(caller) = caller {
                report_completion(&service, &caller, &monitor_tab, &summary);
            }
        }).await;
    });
    Ok(CliLaunch {
        tab_id,
        launch_state: "command_dispatched",
        warnings: prerequisite_hint.into_iter().collect(),
    })
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
            "completion deadline exceeded; inspect the utility tab".into()
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completion_routes_shells_to_records_and_agents_to_reports() {
        assert_eq!(completion_route(None, false), CompletionRoute::SystemLog);
        assert_eq!(
            completion_route(Some("fixture"), false),
            CompletionRoute::AgentReport
        );
        assert_eq!(completion_route(None, true), CompletionRoute::AgentReport);
    }
    #[test]
    fn failed_install_completion_keeps_missing_prerequisite_guidance() {
        let mut status = codeterm_plugin::provider_cli::no_cli();
        status.required = true;
        status.installed = Some(false);
        status.installer_prerequisites = vec![codeterm_plugin::provider_cli::CommandPrerequisite {
            binary: "fixture-runner".into(),
            installed: Some(false),
            hint: "opaque setup guidance".into(),
        }];
        let summary = completion_summary(
            "fixture",
            CliOperation::Install,
            Some(ExecCompletion::CommandFinished {
                exit_code: Some(127),
            }),
            Some(&status),
        );
        assert!(summary.contains("fixture-runner"));
        assert!(summary.contains("opaque setup guidance"));
        assert!(summary.contains("exit code 127"));
    }
    #[test]
    fn no_cli_or_unsupported_os_refuses_before_launch() {
        let mut status = codeterm_plugin::provider_cli::no_cli();
        assert!(operation_command(&status, CliOperation::Install, "linux").is_err());
        status.required = true;
        status.installer = Some("opaque installer".into());
        assert_eq!(
            operation_command(&status, CliOperation::Install, "linux").unwrap(),
            "opaque installer"
        );
        assert!(operation_command(&status, CliOperation::Update, "linux").is_err());
        status.updater = Some("opaque updater".into());
        assert_eq!(
            operation_command(&status, CliOperation::Update, "linux").unwrap(),
            "opaque updater"
        );
    }
    #[test]
    fn result_never_confuses_dispatch_timeout_or_failure_with_success() {
        let mut status = codeterm_plugin::provider_cli::no_cli();
        status.required = true;
        status.installed = Some(false);
        assert!(completion_summary(
            "fixture",
            CliOperation::Install,
            Some(ExecCompletion::CommandFinished { exit_code: Some(0) }),
            Some(&status)
        )
        .contains("still missing"));
        assert!(completion_summary(
            "fixture",
            CliOperation::Update,
            Some(ExecCompletion::CommandFinished { exit_code: Some(7) }),
            Some(&status)
        )
        .contains("exit code 7"));
        assert!(
            completion_summary("fixture", CliOperation::Install, None, None)
                .contains("deadline exceeded")
        );
    }
}
