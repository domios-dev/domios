use std::sync::Arc;

use codeterm_analytics::{CloudSleepReason, Event};
use parking_lot::Mutex;

use super::tests::{
    temp_controller, FakeClock, FakeEnrollment, FakePeerControl, FakeProvisioningPort,
};
use super::*;

#[tokio::test]
async fn cloud_lifecycle_product_analytics_emits_once_per_transition_with_reason() {
    let clock = FakeClock::new(10_000);
    let (_dir, ctrl) = temp_controller(clock.clone());
    let prov = FakeProvisioningPort::new(CloudSleepMode::Stop, clock.clone());
    let enrollment = FakeEnrollment::ok("inst-analytics", "peer-analytics");
    let peer_ctrl = FakePeerControl::new();

    let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    ctrl.bind_product_observer(Arc::new(move |event| {
        sink.lock().push(event);
    }));

    // Refused create emits nothing.
    prov.create_outcomes
        .lock()
        .push_back(Err(CloudError::CreditsExhausted));
    let err = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("analytics-box".into()),
                ..Default::default()
            },
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "credits_exhausted");
    assert!(events.lock().is_empty());

    // Successful create emits cloud_machine_created once.
    let created = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                name: Some("analytics-box".into()),
                ..Default::default()
            },
            prov.as_ref(),
            enrollment.as_ref(),
            peer_ctrl.as_ref(),
        )
        .await
        .unwrap();
    assert_eq!(*events.lock(), vec![Event::CloudMachineCreated {}]);
    events.lock().clear();

    // Manual sleep emits reason=manual once; sleeping an already-asleep machine is a no-op.
    ctrl.sleep(&created.machine_id, prov.as_ref(), peer_ctrl.as_ref(), true)
        .await
        .unwrap();
    ctrl.sleep(&created.machine_id, prov.as_ref(), peer_ctrl.as_ref(), true)
        .await
        .unwrap();
    assert_eq!(
        *events.lock(),
        vec![Event::CloudMachineSlept {
            reason: CloudSleepReason::Manual,
        }]
    );
    events.lock().clear();

    // Wake emits cloud_machine_woken once; waking an already-awake machine is a no-op.
    ctrl.wake(
        &created.machine_id,
        prov.as_ref(),
        peer_ctrl.as_ref(),
        60_000,
    )
    .await
    .unwrap();
    ctrl.wake(
        &created.machine_id,
        prov.as_ref(),
        peer_ctrl.as_ref(),
        60_000,
    )
    .await
    .unwrap();
    assert_eq!(*events.lock(), vec![Event::CloudMachineWoken {}]);
    events.lock().clear();

    // Idle sleep emits reason=idle.
    *peer_ctrl.busy.lock() = PeerBusySnapshot::default();
    ctrl.evaluate_idle_tick(
        &created.machine_id,
        PeerBusySnapshot::default(),
        15 * 60_000,
        DEFAULT_LEASE_RENEW_LEAD_MS,
        None,
        true,
        prov.as_ref(),
        peer_ctrl.as_ref(),
    )
    .await
    .unwrap();
    clock.advance_ms(15 * 60_000);
    ctrl.evaluate_idle_tick(
        &created.machine_id,
        PeerBusySnapshot::default(),
        15 * 60_000,
        DEFAULT_LEASE_RENEW_LEAD_MS,
        None,
        true,
        prov.as_ref(),
        peer_ctrl.as_ref(),
    )
    .await
    .unwrap();
    assert_eq!(
        *events.lock(),
        vec![Event::CloudMachineSlept {
            reason: CloudSleepReason::Idle,
        }]
    );
    events.lock().clear();

    // Wake again, then exhaust lease -> emits reason=exhausted.
    ctrl.wake(
        &created.machine_id,
        prov.as_ref(),
        peer_ctrl.as_ref(),
        60_000,
    )
    .await
    .unwrap();
    assert_eq!(*events.lock(), vec![Event::CloudMachineWoken {}]);
    events.lock().clear();

    let busy_turn = PeerBusySnapshot {
        active_agent_turns: 1,
        running_background_jobs: 0,
        held_sends: 0,
        attached_viewers: 0,
    };
    *peer_ctrl.busy.lock() = busy_turn;
    prov.renew_outcomes
        .lock()
        .push_back(Err(CloudError::CreditsExhausted));
    clock.advance_ms(480_000);
    ctrl.evaluate_idle_tick(
        &created.machine_id,
        busy_turn,
        15 * 60_000,
        DEFAULT_LEASE_RENEW_LEAD_MS,
        None,
        true,
        prov.as_ref(),
        peer_ctrl.as_ref(),
    )
    .await
    .unwrap();
    clock.advance_ms(120_000);
    ctrl.evaluate_idle_tick(
        &created.machine_id,
        busy_turn,
        15 * 60_000,
        DEFAULT_LEASE_RENEW_LEAD_MS,
        None,
        true,
        prov.as_ref(),
        peer_ctrl.as_ref(),
    )
    .await
    .unwrap();
    assert_eq!(
        *events.lock(),
        vec![Event::CloudMachineSlept {
            reason: CloudSleepReason::Exhausted,
        }]
    );
}
