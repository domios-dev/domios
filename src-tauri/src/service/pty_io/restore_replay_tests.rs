use super::lifecycle::{restore_materialization_core, restored_snapshot_launch_input};
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{
    DeliveryAttemptProvenance, PendingDeliveryChannel, PendingDeliveryRecord,
};
use crate::service::boot_stage::BootStage;
use codeterm_util::lock_hold::TimedRwLock;
use std::sync::Arc;

#[test]
fn restored_launch_input_filters_uncertain_turn_from_recording_sink() {
    let historical = "opaque-historical-user-turn";
    let bootstrap = "opaque-new-launch-identity";
    let mut pending =
        PendingDeliveryRecord::new(historical.into(), PendingDeliveryChannel::InteractiveInput);
    pending
        .begin_attempt(DeliveryAttemptProvenance {
            attempt_id: "old-attempt".into(),
            started_at_ms: 1,
        })
        .unwrap();
    pending.mark_unconfirmed();
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("used-session".into());
    tab.cwd = Some("fixture-cwd".into());
    tab.restored_from_snapshot = true;
    tab.pending_delivery = Some(pending.clone());
    tab.restorable_launch.captured = true;
    tab.restorable_launch.identity_instructions = Some(bootstrap.into());
    let mux: MuxState = Arc::new(TimedRwLock::new("restore-no-replay", state));
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_system_prompt_flag(
            "fixture-provider",
            "--fixture-prompt",
        ),
    ));
    let submitted = parking_lot::Mutex::new(Vec::new());
    let launch = restore_materialization_core(
        &mux,
        &tab_id,
        "fixture-provider",
        Some("used-session"),
        Some("fixture-cwd"),
        true,
        super::lifecycle::restore_payload::resolve_restored_payload,
        |_, _, _| panic!("uncertain historical turn cannot fail launch"),
        |launch, payload| {
            let input = restored_snapshot_launch_input(&registry, launch, payload, None);
            if let Some(task) = input.launch_task {
                super::submit_transaction::submit_entry(
                    super::submit_transaction::PtySubmitEntry::Direct,
                    &task,
                    |body| {
                        submitted.lock().push(body.to_string());
                        Ok::<_, ()>(())
                    },
                )
                .unwrap();
            }
            assert!(input.fallback_spec.is_none());
        },
    )
    .unwrap();
    assert!(launch.is_some());
    assert_eq!(submitted.lock().as_slice(), [bootstrap]);
    assert!(!submitted
        .lock()
        .iter()
        .any(|body| body.contains(historical)));
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .pending_delivery
            .as_ref(),
        Some(&pending)
    );
}

#[test]
fn restored_pending_input_refuses_launch_and_preserves_record() {
    let pending = PendingDeliveryRecord::new(
        "missing-body".into(),
        PendingDeliveryChannel::InteractiveInput,
    );
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.restored_from_snapshot = true;
    tab.pending_delivery = Some(pending.clone());
    let mux: MuxState = Arc::new(TimedRwLock::new("restore-pending-no-replay", state));
    let events = parking_lot::Mutex::new(Vec::new());
    assert!(restore_materialization_core(
        &mux,
        &tab_id,
        "fixture-provider",
        None,
        None,
        false,
        super::lifecycle::restore_payload::resolve_restored_payload,
        |stage, _, _| events.lock().push(stage),
        |_, _| panic!("retained user turn must not reach launch submit")
    )
    .is_err());
    assert_eq!(events.lock().as_slice(), [BootStage::SpawnFailed]);
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .pending_delivery
            .as_ref(),
        Some(&pending)
    );
}
