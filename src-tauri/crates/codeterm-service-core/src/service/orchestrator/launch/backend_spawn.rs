use super::*;

pub(super) fn spawn_backend_pty(
    svc: &ServiceContext,
    tab_id: &str,
    launch_env: &std::collections::HashMap<String, String>,
    provider_id: &str,
    direct: Option<(&str, &codeterm_types::agent::LaunchSpec)>,
) -> Result<BackendPty, LaunchFailure> {
    let cwd = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.cwd.clone());
    let mut env_vars = svc.tab_env(tab_id);
    env_vars.extend(launch_env.clone());
    env_vars.extend(
        svc.model_provider_launch_env(provider_id)
            .map_err(|error| LaunchFailure::PtySpawn(error.to_string()))?,
    );
    env_vars.extend(svc.tab_env(tab_id));
    crate::service::agent_restart::timing::stage(tab_id, "pending", "relaunch-pty-request");
    let (pty_id, _) = {
        let gate = svc.pty.lock().spawn_gate();
        let built = match direct {
            Some((pty_id, spec)) => {
                let resolved = codeterm_plugin::provider::resolve_launch_spec(spec)
                    .map_err(LaunchFailure::PtySpawn)?;
                crate::startup::boot_timing::mark(
                    tab_id,
                    Some(pty_id),
                    match resolved.interpreter {
                        codeterm_plugin::provider::LaunchInterpreter::Direct => "direct_executable",
                        codeterm_plugin::provider::LaunchInterpreter::Command => {
                            "direct_cmd_interpreter"
                        }
                        codeterm_plugin::provider::LaunchInterpreter::PowerShell => {
                            "direct_powershell_interpreter"
                        }
                    },
                );
                let nonce = svc
                    .mux
                    .read()
                    .get_tab(tab_id)
                    .and_then(|tab| tab.session_binding_nonce.clone())
                    .ok_or_else(|| LaunchFailure::PtySpawn("direct launch nonce missing".into()))?;
                let env_vars = super::super::launch_env::direct_environment(
                    env_vars,
                    &resolved.spec,
                    &nonce,
                    pty_id,
                    crate::service::instance_env::instance_cli_binary_path().as_deref(),
                    &std::env::var_os("PATH").unwrap_or_default(),
                )
                .map_err(LaunchFailure::PtySpawn)?;
                crate::pty::manager::build_direct_session(
                    gate,
                    crate::pty::shell_profiles::ShellLaunch {
                        executable: resolved.spec.program,
                        args: resolved.spec.args,
                        cwd,
                    },
                    120,
                    40,
                    env_vars,
                    &resolved.spec.env_unset,
                )
            }
            None => crate::pty::manager::build_session(gate, None, cwd, 120, 40, env_vars),
        };
        built.map(|built| {
            svc.pty
                .lock()
                .register_session(direct.map(|(id, _)| id.to_owned()), built)
        })
    }
    .map_err(|error| LaunchFailure::PtySpawn(error.to_string()))?;
    crate::service::agent_restart::timing::stage(tab_id, &pty_id, "relaunch-pty-created");
    crate::service::pty_resize::resize(&svc.pty, &svc.screen_state, &pty_id, None)
        .map_err(LaunchFailure::PtySpawn)?;
    if direct.is_some() {
        if let Some(attempt) =
            crate::service::launch_readiness::LaunchAttempt::current(&svc.mux, tab_id, &pty_id)
        {
            DIRECT_LAUNCHES.lock().insert(pty_id.clone(), attempt);
        }
    }
    let reader = match start_backend_reader(svc, &pty_id) {
        Ok(reader) => reader,
        Err(error) => {
            DIRECT_LAUNCHES.lock().remove(&pty_id);
            return Err(error);
        }
    };

    Ok(BackendPty { id: pty_id, reader })
}

static DIRECT_LAUNCHES: std::sync::LazyLock<
    parking_lot::Mutex<
        std::collections::HashMap<String, crate::service::launch_readiness::LaunchAttempt>,
    >,
> = std::sync::LazyLock::new(Default::default);

pub(super) fn reader_finished(svc: &ServiceContext, pty_id: &str) {
    if !DIRECT_LAUNCHES.lock().contains_key(pty_id) {
        return;
    }
    let session = svc.pty.lock().get_session(pty_id);
    let exited = session.as_ref().is_some_and(|session| {
        session
            .child_exit
            .wait_resolved(std::time::Duration::from_secs(5))
            .is_some()
            && session
                .child
                .lock()
                .is_ok_and(|mut child| matches!(child.try_wait(), Ok(Some(_))))
    });
    let current = {
        let mux = svc.mux.read();
        mux.tab_id_for_pty(pty_id)
            .and_then(|tab_id| mux.get_tab(&tab_id).cloned())
    };
    let Some(attempt) = take_direct_recovery(
        &mut DIRECT_LAUNCHES.lock(),
        pty_id,
        current.as_ref(),
        exited,
        crate::crash::shutdown::active_shutdown().is_some(),
    ) else {
        return;
    };
    let claim = svc.pty_ownership.acquire(&attempt.tab_id);
    let ready = svc
        .mux
        .read()
        .get_tab(&attempt.tab_id)
        .is_some_and(|tab| direct_exit_owns_tab(tab, &attempt));
    if !ready || crate::crash::shutdown::active_shutdown().is_some() {
        return;
    }
    if !svc.clear_tab_agent_identity_after_confirmed_exit(
        &attempt.tab_id,
        "direct provider PTY exited",
    ) {
        return;
    }
    drop(claim);
    if let Err(error) = svc.recover_exited_provider_after_launch_exit(&attempt) {
        log::warn!(
            "[direct-launch] exit recovery refused tab={} pty={}: {error:?}",
            attempt.tab_id,
            attempt.pty_id
        );
    }
}

fn direct_exit_owns_tab(
    tab: &crate::multiplexer::types::Tab,
    attempt: &crate::service::launch_readiness::LaunchAttempt,
) -> bool {
    crate::service::launch_readiness::attempt_owns_tab(tab, attempt)
        && tab.launch_session_authenticated()
        && tab.boot_stage == Some(BootStage::AgentReady)
}

fn take_direct_recovery(
    launches: &mut std::collections::HashMap<
        String,
        crate::service::launch_readiness::LaunchAttempt,
    >,
    pty_id: &str,
    tab: Option<&crate::multiplexer::types::Tab>,
    confirmed_exit: bool,
    shutting_down: bool,
) -> Option<crate::service::launch_readiness::LaunchAttempt> {
    let attempt = launches.remove(pty_id)?;
    (confirmed_exit && !shutting_down && tab.is_some_and(|tab| direct_exit_owns_tab(tab, &attempt)))
        .then_some(attempt)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn arm_direct_launch(
    svc: &ServiceContext,
    launch: &mut LaunchGeneration,
    claim: &crate::service::pty_ownership::TabPtyClaim,
    displaced: Option<&str>,
    task_identity: Option<&crate::service::startup_lease::StartupTaskIdentity>,
    public_send_key: Option<crate::service::public_send_readiness::PublicSendGateKey>,
    origin: crate::service::launch_diagnostics::AgentLaunchOrigin,
    resume_session_id: Option<&str>,
    reserved_nonce: Option<String>,
) -> Result<(String, generation::ReadinessRegistration), LaunchFailure> {
    let pty_id = uuid::Uuid::new_v4().to_string();
    let selected =
        svc.link_provider_launch_pty(claim, &launch.tab_id, pty_id.clone(), displaced, true);
    if !selected.linked || selected.pty_id != pty_id || !launch.replace(pty_id.clone()) {
        return Err(LaunchFailure::TabClosed);
    }
    launch
        .capture_binding_nonce(
            svc,
            task_identity,
            public_send_key,
            origin,
            resume_session_id,
            reserved_nonce,
        )
        .map_err(LaunchFailure::StartupSupervision)?;
    let registration = generation::ReadinessRegistration::new(
        launch.attempt().expect("direct launch binding nonce"),
    );
    Ok((pty_id, registration))
}

pub(super) fn mark_direct_reader_ready(
    launch: &mut LaunchGeneration,
    svc: &ServiceContext,
    delivery: &mut crate::service::launch_delivery::LaunchDeliveryGate,
) -> bool {
    if !launch.emit(svc, BootStage::ShellReady) {
        return false;
    }
    delivery.mark_shell_ready();
    launch.emit(svc, BootStage::AgentLaunching) && launch.emit(svc, BootStage::LaunchWritten)
}

#[cfg(test)]
#[path = "backend_spawn/tests.rs"]
mod tests;
