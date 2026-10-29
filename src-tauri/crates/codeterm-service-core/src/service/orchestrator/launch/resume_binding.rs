use super::{LaunchGeneration, LAUNCH_REGISTRY};
use crate::service::launch_diagnostics::AgentLaunchOrigin;
use crate::service::{boot_stage::BootStage, ServiceContext};

fn recovery_required(relaunch: bool, authenticated: bool, current: bool) -> bool {
    relaunch && !authenticated && current
}

pub(super) fn finish_unbound_relaunch(
    launch: &LaunchGeneration,
    svc: &ServiceContext,
    origin: AgentLaunchOrigin,
) -> Option<bool> {
    if !origin.relaunches_existing_agent() {
        return None;
    }
    if svc.resumed_provider_is_live(&launch.tab_id, &launch.pty_id) {
        if let Some(attempt) = launch.attempt() {
            svc.record_provisional_launch(
                &attempt,
                BootStage::LaunchBindingUnconfirmed,
                "live resumed provider awaits exact session evidence",
            );
        }
        return Some(false);
    }
    let Some(attempt) = launch.attempt() else {
        return Some(false);
    };
    if !LAUNCH_REGISTRY.lock().finish(launch) {
        return Some(false);
    }
    svc.retire_public_send_launch_attempt(&attempt);
    let (authenticated, current) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(&attempt.tab_id);
        (
            tab.is_some_and(|tab| tab.launch_session_authenticated()),
            tab.is_some_and(|tab| {
                tab.pty_id.as_deref() == Some(attempt.pty_id.as_str())
                    && tab.session_binding_nonce.as_deref() == Some(attempt.binding_nonce.as_str())
            }),
        )
    };
    if !recovery_required(origin.relaunches_existing_agent(), authenticated, current) {
        return Some(authenticated && current);
    }
    let reason = "restored agent session binding is unconfirmed; explicit recovery required";
    svc.record_provisional_launch(&attempt, BootStage::LaunchBindingUnconfirmed, reason);
    svc.log_system_notice(
        "Orchestration incident: restored agent needs recovery".into(),
        Some(format!(
            "Impact: Tab {} has no authenticated session binding and cannot act as a Team member. Cleanup: its launch was retired; written bootstrap input will not be replayed. Fallback: inspect this Tab and recover it explicitly with `domios agent restart --tab {}`. Cause: {reason}.",
            attempt.tab_id, attempt.tab_id,
        )),
        Some("error"),
    );
    Some(false)
}

#[cfg(test)]
mod tests;
