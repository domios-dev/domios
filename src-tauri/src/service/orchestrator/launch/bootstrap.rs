use super::resume_binding;
use crate::service::{
    launch_delivery::{FallbackDeliverySpec, LaunchConsumptionOutcome, LaunchDeliveryGate},
    launch_input_gate::LaunchInputEvidence,
    launch_readiness::LaunchAttempt,
    ServiceContext,
};

pub(super) fn consume(
    svc: &ServiceContext,
    attempt: &LaunchAttempt,
    provider: &str,
    spec: FallbackDeliverySpec,
    delivery_gate: &LaunchDeliveryGate,
    launch_input_readiness: Option<LaunchInputEvidence>,
    binding_turn_planned: bool,
) -> Result<bool, String> {
    let tab_id = &attempt.tab_id;
    let pty_id = &attempt.pty_id;
    if LaunchAttempt::current(&svc.mux, tab_id, pty_id).as_ref() != Some(attempt) {
        return Err("launch bootstrap authority was superseded".into());
    }
    if crate::service::codex_rpc::lifecycle::for_tab(tab_id).is_some() {
        return crate::service::codex_rpc::bootstrap::consume(svc, attempt, spec);
    }
    let agent_cmd = provider;
    {
        let mut mux = svc.mux.write();
        let tab = mux.get_tab_mut(tab_id).expect("launch Tab exists");
        if binding_turn_planned {
            resume_binding::prepare_binding_turn(tab, &spec);
        }
        tab.pending_delivery.get_or_insert_with(|| {
            crate::service::launch_delivery::pending_record_for_spec(spec.clone())
        });
    }
    let submit_strategy = svc
        .agent_registry
        .with_provider(&agent_cmd, |provider| provider.verify_submit_strategy())
        .unwrap_or(crate::agent::provider::VerifySubmitStrategy::None);
    let launch_frame_prefix = svc
        .agent_registry
        .with_provider(&agent_cmd, |provider| {
            crate::service::launch_delivery::launch_input_frame_prefix(
                provider.composer_cursor_position(),
            )
        })
        .flatten();
    let consumption = delivery_gate.consume_injection_with_readiness(
        &svc.mux,
        tab_id,
        svc,
        launch_input_readiness,
        || {
            crate::service::pty_io::submit_transaction::submit_text_with_key_claimed_and_frame(
                svc,
                pty_id,
                &spec.payload,
                launch_frame_prefix,
                None,
                None,
                true,
                b"\r",
                submit_strategy,
                crate::service::inject_verify::VerifyOpts {
                    require_prompt_first: false,
                    verify_echo: true,
                    verify_submit: true,
                    ..Default::default()
                },
                None,
                false,
                crate::service::pty_io::submit_transaction::claim_submit_cycle(pty_id),
            )
        },
    );

    match consumption {
        Ok(receipt) if receipt.outcome == LaunchConsumptionOutcome::Confirmed => {
            svc.confirm_pending_delivery(
                tab_id,
                receipt
                    .attempt_id
                    .as_deref()
                    .expect("confirmed launch attempt"),
            )
            .map_err(|e| e.to_string())?;
            Ok(true)
        }
        Ok(receipt) => Ok(receipt.outcome == LaunchConsumptionOutcome::AlreadyConfirmed),
        Err(error) if error.surfaces_as_launch_failure() => Err(error.to_string()),
        Err(error) => {
            log::warn!("[launch-delivery] tab={tab_id} pty={pty_id} state={} bytes_written=true; pending evidence retained", error.delivery.code());
            Ok(false)
        }
    }
}

const DRAFT_NUDGE_ACK_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

pub(super) fn unconfirmed_draft_nudge_due(
    bootstrap_consumed: bool,
    first_turn_launch: bool,
    settlement: &crate::service::launch_readiness::LaunchSettlement,
    session_authenticated: bool,
) -> bool {
    !bootstrap_consumed
        && first_turn_launch
        && !session_authenticated
        && matches!(
            settlement,
            crate::service::launch_readiness::LaunchSettlement::Provisional { .. }
        )
}

/// Before an unbound launch fails, press Enter once on a first-turn draft still
/// parked in the composer, then give the binding a short window to arrive.
pub(super) fn nudge_unconfirmed_draft(
    svc: &ServiceContext,
    attempt: &LaunchAttempt,
    draft: &str,
    await_ack: impl FnOnce(std::time::Duration) -> bool,
) -> bool {
    if LaunchAttempt::current(&svc.mux, &attempt.tab_id, &attempt.pty_id).as_ref() != Some(attempt)
    {
        return false;
    }
    match crate::service::pty_io::submit_transaction::nudge_unbound_launch_draft(
        svc,
        &attempt.pty_id,
        draft,
        b"\r",
    ) {
        Ok(()) => {
            let bound = await_ack(DRAFT_NUDGE_ACK_WAIT);
            log::info!(
                "[launch-delivery] tab={} pty={} draft-nudge sent; session_bound={}",
                attempt.tab_id,
                attempt.pty_id,
                bound
            );
            bound
        }
        Err(reason) => {
            log::info!(
                "[launch-delivery] tab={} pty={} draft-nudge skipped: {}",
                attempt.tab_id,
                attempt.pty_id,
                reason
            );
            false
        }
    }
}
