use super::{LaunchFailure, LaunchGeneration, LAUNCH_REGISTRY};
use crate::service::boot_stage::BootStage;
use crate::service::launch_readiness::LaunchSettlement;
use crate::service::ServiceContext;

#[cfg(any(test, feature = "test-support"))]
pub fn launch_waits_for_first_turn_binding(
    svc: &ServiceContext,
    tab_id: &str,
    agent_cmd: &str,
    has_startup_task_turn: bool,
    origin: crate::service::launch_diagnostics::AgentLaunchOrigin,
) -> bool {
    let _ = (tab_id, origin);
    launch_waits_for_first_turn_input(svc, agent_cmd, has_startup_task_turn)
}

pub(super) fn launch_waits_for_first_turn_input(
    svc: &ServiceContext,
    agent_cmd: &str,
    has_startup_task_turn: bool,
) -> bool {
    let lazy = svc
        .agent_registry
        .with_provider(agent_cmd, |provider| {
            provider.session_starts_on_first_user_turn()
        })
        .unwrap_or(false);
    first_turn_binding_required(lazy, has_startup_task_turn)
}

pub(super) fn first_turn_binding_required(lazy: bool, has_startup_task_turn: bool) -> bool {
    lazy && !has_startup_task_turn
}

pub(super) fn provider_ready_stage(
    settlement: &LaunchSettlement,
    session_authenticated: bool,
) -> Option<BootStage> {
    settlement
        .authorizes_agent_ready(session_authenticated)
        .then_some(BootStage::ProviderReady)
}

pub(super) fn finish_waiting_for_first_turn(
    launch: &mut LaunchGeneration,
    settlement: &LaunchSettlement,
    svc: &ServiceContext,
    origin: crate::service::launch_diagnostics::AgentLaunchOrigin,
) -> Option<bool> {
    if !matches!(settlement, LaunchSettlement::AwaitingFirstTurn) {
        return None;
    }
    let input_ready = svc.mux.read().get_tab(&launch.tab_id).is_some_and(|tab| {
        launch.attempt().as_ref().is_some_and(|attempt| {
            crate::service::launch_readiness::attempt_owns_tab(tab, attempt)
                && crate::service::boot_stage::first_turn_input_is_ready(tab)
        })
    });
    if !input_ready {
        return super::resume_binding::finish_unbound_relaunch(launch, svc, origin).or(Some(false));
    }
    let finished = LAUNCH_REGISTRY.lock().finish(launch);
    if let Some(attempt) = launch.attempt() {
        svc.retire_public_send_launch_attempt(&attempt);
    }
    if finished {
        log::info!(
            "[launch-readiness] Tab {} remains available for its first turn",
            launch.tab_id
        );
    }
    Some(finished)
}

pub(super) fn reused_provider_tui_owns_launch(
    reused_live_pty: bool,
    session_authenticated: bool,
    tab_agent_type: Option<&str>,
    requested_agent: &str,
    provider_tui_visible: bool,
) -> bool {
    reused_live_pty
        && session_authenticated
        && tab_agent_type == Some(requested_agent)
        && provider_tui_visible
}

pub(super) fn emit_agent_ready(
    launch: &mut LaunchGeneration,
    svc: &ServiceContext,
    settlement: LaunchSettlement,
) -> Result<(), String> {
    let session_authenticated = launch.reconcile_authenticated_binding(svc);
    if !settlement.authorizes_agent_ready(session_authenticated) {
        if let Some(attempt) = launch.attempt() {
            svc.retire_public_send_launch_attempt(&attempt);
        }
        return Err(match settlement {
            LaunchSettlement::Failed { reason, .. } => {
                LAUNCH_REGISTRY.lock().finish(launch);
                reason
            }
            LaunchSettlement::OwnerPromptDeadline { reason } => {
                fail_unbound_launch(launch, svc, reason)
            }
            LaunchSettlement::Superseded => {
                LAUNCH_REGISTRY.lock().finish(launch);
                "launch readiness was superseded".into()
            }
            LaunchSettlement::AwaitingFirstTurn => {
                LAUNCH_REGISTRY.lock().finish(launch);
                "launch awaits its first turn".into()
            }
            LaunchSettlement::Ready => fail_unbound_launch(
                launch,
                svc,
                "the launch owner did not receive the authenticated binding".into(),
            ),
            LaunchSettlement::Provisional { reason, .. } => {
                fail_unbound_launch(launch, svc, reason)
            }
        });
    }
    if !launch.emit(svc, BootStage::AgentReady) {
        return Err(fail_unbound_launch(
            launch,
            svc,
            "the launch lost its authenticated session before readiness".into(),
        ));
    }
    Ok(())
}

fn fail_unbound_launch(launch: &LaunchGeneration, svc: &ServiceContext, reason: String) -> String {
    let failure = LaunchFailure::SessionBindingNotObserved(reason);
    let error = failure.reason();
    launch.fail(svc, failure);
    error
}
