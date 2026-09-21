use super::{LaunchFailure, LaunchGeneration, LAUNCH_REGISTRY};
use crate::service::boot_stage::BootStage;
use crate::service::launch_readiness::LaunchSettlement;
use crate::service::ServiceContext;
use std::time::Duration;

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

fn fail_unbound_launch(
    launch: &LaunchGeneration,
    svc: &ServiceContext,
    reason: String,
) -> String {
    let failure = LaunchFailure::SessionBindingNotObserved(reason);
    let error = failure.reason();
    launch.fail(svc, failure);
    error
}
