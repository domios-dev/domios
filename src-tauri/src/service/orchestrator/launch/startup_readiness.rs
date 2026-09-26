use super::{LaunchFailure, LaunchGeneration, LAUNCH_REGISTRY};
use crate::service::boot_stage::BootStage;
use crate::service::launch_readiness::{
    attempt_owns_tab, launch_input_evidence, LaunchAttempt, LaunchInputEvidence,
    LaunchInputRefusal, LaunchSettlement,
};
use crate::service::ServiceContext;
use std::time::Duration;

pub(super) fn await_launch_input_readiness(
    launch: &LaunchGeneration,
    svc: &ServiceContext,
    attempt: &LaunchAttempt,
    timeout: Duration,
    resumes_on_first_turn: bool,
) -> Result<LaunchInputEvidence, LaunchInputRefusal> {
    if let Some(evidence) = current_launch_input_evidence(svc, attempt, resumes_on_first_turn)? {
        return Ok(evidence);
    }
    if !launch.await_provider_ack(timeout) {
        return Err(LaunchInputRefusal::Deadline);
    }
    current_launch_input_evidence(svc, attempt, resumes_on_first_turn)?
        .ok_or(LaunchInputRefusal::SessionBindingNotAuthenticated)
}

fn current_launch_input_evidence(
    svc: &ServiceContext,
    attempt: &LaunchAttempt,
    resumes_on_first_turn: bool,
) -> Result<Option<LaunchInputEvidence>, LaunchInputRefusal> {
    let mux = svc.mux.read();
    let Some(tab) = mux.get_tab(&attempt.tab_id) else {
        return Err(LaunchInputRefusal::AttemptSuperseded);
    };
    if !attempt_owns_tab(tab, attempt) {
        return Err(LaunchInputRefusal::AttemptSuperseded);
    }
    Ok(launch_input_evidence(
        tab.launch_session_authenticated(),
        resumes_on_first_turn,
    ))
}

pub(super) fn resume_waits_for_first_turn(
    svc: &ServiceContext,
    tab_id: &str,
    agent_cmd: &str,
    origin: crate::service::launch_diagnostics::AgentLaunchOrigin,
) -> bool {
    if !matches!(
        origin,
        crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot
    ) {
        return false;
    }
    let has_saved_session = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.agent_session_id.as_deref())
        .is_some_and(|session_id| !session_id.trim().is_empty());
    has_saved_session
        && svc
            .agent_registry
            .with_provider(agent_cmd, |provider| {
                provider.resume_binds_on_first_user_turn()
            })
            .unwrap_or(false)
}

pub(super) fn input_ready_stage_after_provider_ready(resumes_on_first_turn: bool) -> BootStage {
    if resumes_on_first_turn {
        BootStage::AgentReady
    } else {
        BootStage::ProviderReady
    }
}

pub(super) fn finish_waiting_for_first_turn(
    launch: &mut LaunchGeneration,
    settlement: &LaunchSettlement,
) -> Option<bool> {
    if !matches!(settlement, LaunchSettlement::AwaitingFirstTurn) {
        return None;
    }
    let finished = LAUNCH_REGISTRY.lock().finish(launch);
    if finished {
        log::info!(
            "[launch-readiness] Tab {} remains available for its resume first turn",
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
    let session_authenticated = launch.await_provider_ack(Duration::ZERO);
    if !settlement.authorizes_agent_ready(session_authenticated) {
        return Err(match settlement {
            LaunchSettlement::Failed { reason, .. } => {
                LAUNCH_REGISTRY.lock().finish(launch);
                reason
            }
            LaunchSettlement::Superseded => {
                LAUNCH_REGISTRY.lock().finish(launch);
                "launch readiness was superseded".into()
            }
            LaunchSettlement::AwaitingFirstTurn => {
                LAUNCH_REGISTRY.lock().finish(launch);
                "resume awaiting first turn".into()
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
