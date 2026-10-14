use std::sync::Arc;
use std::time::Duration;

use crate::agent::provider::AgentLaunchParams;
use crate::service::launch_diagnostics::AgentLaunchOrigin;
use crate::service::launch_readiness::{self, LaunchAttempt};
use crate::service::ServiceContext;

const SHELL_READY_DEADLINE: Duration = Duration::from_secs(10);

pub(crate) fn first_turn(
    system_prompt: bool,
    inline_task: bool,
    task: Option<&str>,
    starter: Option<&str>,
) -> Option<String> {
    let task = task.filter(|task| !task.trim().is_empty())?;
    if system_prompt {
        Some(super::launch_delivery::assigned_task_starter(starter))
    } else if inline_task {
        None
    } else {
        Some(task.to_owned())
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ShellReadyDeadline;

pub(crate) struct HiddenLaunch {
    pub(crate) tab_id: String,
    pub(crate) pty_id: String,
    pub(crate) provider: String,
    pub(crate) launch_session_id: Option<String>,
    pub(crate) task: Option<String>,
    pub(crate) model_args: Vec<String>,
    pub(crate) selection: crate::ports::transport::Selection,
    pub(crate) role: Option<String>,
    pub(crate) role_profile: Option<String>,
    pub(crate) pty_input_task: Option<String>,
    pub(crate) permission_bypass: bool,
    pub(crate) cron_run: bool,
}

fn after_shell_ready<T>(
    cron_run: bool,
    screen_state: &crate::ScreenStateHandle,
    pty_id: &str,
    deadline: Duration,
    launch: impl FnOnce() -> T,
) -> Result<T, ShellReadyDeadline> {
    let ready = crate::service::pty_io::await_shell_prompt(screen_state, pty_id, deadline);
    if cron_run && !ready {
        return Err(ShellReadyDeadline);
    }
    Ok(launch())
}

pub(crate) fn start(service: Arc<ServiceContext>, launch: HiddenLaunch) {
    let HiddenLaunch {
        tab_id,
        pty_id,
        provider,
        launch_session_id,
        task,
        model_args,
        selection,
        role,
        role_profile,
        pty_input_task,
        permission_bypass,
        cron_run,
    } = launch;
    std::thread::spawn(move || {
        let cron_attempt = if cron_run {
            let Some(attempt) = LaunchAttempt::current(&service.mux, &tab_id, &pty_id) else {
                service.run_watcher.fail_now(
                    &service,
                    &pty_id,
                    "cron launch attempt unavailable before shell readiness".into(),
                );
                return;
            };
            launch_readiness::arm_cron_run(attempt.clone());
            Some(attempt)
        } else {
            None
        };

        let reader_service = service.clone();
        let reader_pty = pty_id.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("backend reader runtime");
            runtime.block_on(async {
                match crate::service::pty_lifecycle::start_backend_reader(
                    &reader_service,
                    &reader_pty,
                    true,
                ) {
                    Ok(handle) => {
                        let _ = handle.await;
                    }
                    Err(error) => log::warn!("[spawn.hidden] backend reader: {error}"),
                }
            });
        });

        let readiness = after_shell_ready(
            cron_run,
            &service.screen_state,
            &pty_id,
            SHELL_READY_DEADLINE,
            || {
                let hidden_cwd = service
                    .mux
                    .read()
                    .get_tab(&tab_id)
                    .and_then(|tab| tab.cwd.clone())
                    .unwrap_or_default();
                let mut launch_params = AgentLaunchParams {
                    context_files: None,
                    tool_less_instructions_path: None,
                    agent_name: None,
                    task: task.clone(),
                    starter_prompt: Default::default(),
                    args: (!model_args.is_empty()).then_some(model_args),
                    skip_permissions: permission_bypass,
                    session_id: launch_session_id,
                    structured_prompt_server: crate::service::structured_prompt::ensure_for_spawn(
                        &service,
                        &provider,
                        &tab_id,
                        &hidden_cwd,
                    ),
                    launch_marker: None,
                    instance_id: Some(crate::paths::codeterm_instance_id().to_string()),
                };
                let launch_cmd = super::launch_session_detection::marked_launch_command_for_tab(
                    &service,
                    &provider,
                    &tab_id,
                    &hidden_cwd,
                    &mut launch_params,
                    selection,
                )
                .unwrap_or_else(|| provider.clone());
                crate::service::launch_diagnostics::log_launch(
                    AgentLaunchOrigin::HiddenSpawn,
                    &tab_id,
                    &provider,
                    &launch_cmd,
                    task.as_deref(),
                    role.as_deref(),
                    role_profile.as_deref(),
                    "provider_descriptor",
                );

                match service.resolve_pty_id(&tab_id) {
                    Ok(launch_pty_id) => {
                        if let Some(attempt) = cron_attempt.as_ref() {
                            if LaunchAttempt::current(&service.mux, &tab_id, &launch_pty_id)
                                .as_ref()
                                != Some(attempt)
                            {
                                service.run_watcher.fail_now(
                                    &service,
                                    &launch_pty_id,
                                    "cron launch attempt changed before provider launch".into(),
                                );
                                return;
                            }
                        }

                        if let Err(error) = service.send_shell_spawn(&launch_pty_id, &launch_cmd) {
                            log::error!("[spawn.hidden] shell spawn failed: {error}");
                            if cron_run {
                                service.run_watcher.fail_now(
                                    &service,
                                    &launch_pty_id,
                                    format!("cron provider launch failed: {error}"),
                                );
                            }
                            return;
                        }
                        if let Some(attempt) = cron_attempt.as_ref() {
                            if !launch_readiness::mark_launch_issued(attempt) {
                                service.run_watcher.fail_now(
                                    &service,
                                    &launch_pty_id,
                                    "cron launch attempt was superseded before provider launch"
                                        .into(),
                                );
                                return;
                            }
                        }

                        if let Some(sidecar) = super::codex_rpc::lifecycle::for_tab(&tab_id) {
                            let nonce = sidecar.launch_nonce.clone();
                            if !super::codex_rpc::lifecycle::wait_attached(
                                &tab_id,
                                &nonce,
                                Duration::from_secs(30),
                            ) {
                                super::codex_rpc::recovery::try_start(
                                    &service,
                                    &tab_id,
                                    &nonce,
                                    "hidden remote TUI attachment failed",
                                );
                                if !super::codex_rpc::recovery::wait_recovery(&tab_id, &nonce) {
                                    return;
                                }
                            }
                            let Some(attempt) =
                                LaunchAttempt::current(&service.mux, &tab_id, &launch_pty_id)
                            else {
                                return;
                            };
                            let rpc_task = service.agent_registry.with_provider(&provider, |p| {
                                first_turn(
                                    matches!(super::launch_delivery::mode_for_descriptor(&p.system_prompt_delivery()), super::launch_delivery::LaunchDeliveryMode::SystemPrompt),
                                    false,
                                    task.as_deref(),
                                    p.starter_prompt_text(crate::agent::provider::StarterPrompt::AssignedTask).as_deref(),
                                )
                            }).flatten();
                            let consumed = match rpc_task.as_ref() {
                                Some(payload) => super::codex_rpc::bootstrap::consume(&service, &attempt, super::launch_delivery::FallbackDeliverySpec {
                                    channel: crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput,
                                    payload: payload.clone(),
                                }),
                                None => Ok(true),
                            };
                            match consumed {
                                Ok(true) => {
                                    super::codex_rpc::lifecycle::startup_complete(&service, &tab_id, &nonce);
                                    service.run_watcher.arm(&launch_pty_id);
                                },
                                result => log::error!("[spawn.hidden] tab={tab_id} startup RPC not confirmed: {result:?}; sends held"),
                            }
                        } else if let Some(task_text) = pty_input_task.as_deref() {
                            if let Err(error) = super::spawn::deliver_hidden_pty_task(
                                &service,
                                &launch_pty_id,
                                &tab_id,
                                &provider,
                                task_text,
                            ) {
                                log::error!(
                                    "[spawn.hidden] task delivery failed for Tab {}: {}",
                                    tab_id,
                                    error
                                );
                                service.run_watcher.fail_now(
                                    &service,
                                    &launch_pty_id,
                                    format!("task delivery failed: {error}"),
                                );
                            } else {
                                service.run_watcher.arm(&launch_pty_id);
                            }
                        } else {
                            service.run_watcher.arm(&launch_pty_id);
                        }
                    }
                    Err(error) => {
                        log::error!("[spawn.hidden] no pty for Tab {}: {}", tab_id, error);
                        if cron_run {
                            service.run_watcher.fail_now(
                                &service,
                                &pty_id,
                                format!("cron PTY unavailable after shell readiness: {error}"),
                            );
                        }
                    }
                }
                log::debug!(
                    "[spawn.hidden] agent launched tab={} provider={}",
                    tab_id,
                    provider
                );
            },
        );
        if readiness.is_err() {
            service.run_watcher.fail_now(
                &service,
                &pty_id,
                "cron shell readiness deadline elapsed before provider launch".into(),
            );
        }
    });
}

#[cfg(test)]
#[path = "spawn_hidden_launch/tests.rs"]
mod tests;
