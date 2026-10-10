use super::{LaunchGeneration, LAUNCH_REGISTRY};
use crate::multiplexer::types::{PendingDeliveryChannel, PendingDeliveryState, Tab};
use crate::service::launch_delivery::FallbackDeliverySpec;
use crate::service::launch_diagnostics::AgentLaunchOrigin;
use crate::service::{boot_stage::BootStage, ServiceContext};

fn binding_turn_required(
    relaunch: bool,
    lazy_session: bool,
    first_turn_planned: bool,
    resume_target: Option<&str>,
) -> bool {
    relaunch && lazy_session && !first_turn_planned && resume_target.is_none()
}

pub(super) fn for_launch(
    svc: &ServiceContext,
    tab_id: &str,
    provider_id: &str,
    origin: AgentLaunchOrigin,
    first_turn_planned: bool,
    resume_target: Option<&str>,
) -> Option<FallbackDeliverySpec> {
    let lazy_session = svc
        .agent_registry
        .with_provider(provider_id, |provider| {
            provider.session_starts_on_first_user_turn()
        })
        .unwrap_or(false);
    let pending = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.pending_delivery.as_ref().map(|pending| pending.state()));
    binding_turn(
        matches!(origin, AgentLaunchOrigin::RestoredSnapshot),
        lazy_session,
        first_turn_planned,
        resume_target,
        pending,
    )
}

pub(super) fn binding_turn(
    relaunch: bool,
    lazy_session: bool,
    first_turn_planned: bool,
    resume_target: Option<&str>,
    pending: Option<PendingDeliveryState>,
) -> Option<FallbackDeliverySpec> {
    (binding_turn_required(relaunch, lazy_session, first_turn_planned, resume_target)
        && pending.is_none_or(|state| state.is_accepted())).then(|| FallbackDeliverySpec {
        channel: PendingDeliveryChannel::FallbackBootstrap,
        payload: "This agent session has been restored. Run `codeterm identify` to verify your connection, then resume your interrupted work. If there is no pending work, wait for the next instruction.".into(),
    })
}

pub(super) fn prepare_binding_turn(tab: &mut Tab, spec: &FallbackDeliverySpec) {
    if tab
        .pending_delivery
        .as_ref()
        .is_none_or(|pending| pending.state().is_accepted())
    {
        tab.pending_delivery = Some(crate::service::launch_delivery::pending_record_for_spec(
            spec.clone(),
        ));
    }
}

fn recovery_required(relaunch: bool, authenticated: bool, current: bool) -> bool {
    relaunch && !authenticated && current
}

pub(super) fn fallback_still_required(binding_only: bool, authenticated: bool) -> bool {
    !binding_only || !authenticated
}

pub(super) fn await_fallback_required(
    launch: &LaunchGeneration,
    binding_only: bool,
    deadline: std::time::Instant,
) -> bool {
    if !binding_only {
        return true;
    }
    let mut authenticated = launch.provider_ack.ready.lock();
    while fallback_still_required(binding_only, *authenticated) {
        if launch
            .provider_ack
            .changed
            .wait_until(&mut authenticated, deadline)
            .timed_out()
        {
            break;
        }
    }
    fallback_still_required(binding_only, *authenticated)
}

pub(super) fn finish_unbound_relaunch(
    launch: &LaunchGeneration,
    svc: &ServiceContext,
    origin: AgentLaunchOrigin,
) -> Option<bool> {
    if !origin.relaunches_existing_agent() {
        return None;
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
    svc.create_notification(
        "Orchestration incident: restored agent needs recovery".into(),
        Some(format!(
            "Impact: Tab {} has no authenticated session binding and cannot act as a Team member. Cleanup: its launch was retired; written bootstrap input will not be replayed. Fallback: inspect this Tab and recover it explicitly with `codeterm agent restart --tab {}`. Cause: {reason}.",
            attempt.tab_id, attempt.tab_id,
        )),
        Some("error"),
    );
    Some(false)
}

#[cfg(test)]
mod tests;
