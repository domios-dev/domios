use super::*;
use std::{future::Future, task::Poll};

#[tokio::test]
async fn paid_wake_readiness_failure_emits_one_billing_refresh_event() {
    let clock = FakeClock::new(1_000_000);
    let (_dir, ctrl) = temp_controller(clock.clone());
    let prov = FakeProvisioningPort::new(CloudSleepMode::DeleteRecreate, clock);
    let enrollment = FakeEnrollment::ok("inst-ready", "peer-ready");
    let peer = FakePeerControl::new();
    let created = ctrl
        .create(
            CreateCloudMachineParams {
                size: "small".into(),
                ..Default::default()
            },
            prov.as_ref(),
            enrollment.as_ref(),
            peer.as_ref(),
        )
        .await
        .unwrap();
    ctrl.sleep(&created.machine_id, prov.as_ref(), peer.as_ref(), true)
        .await
        .unwrap();
    *peer.readiness_error.lock() = Some(CloudError::WakeDeadlineExceeded {
        machine_id: created.machine_id.clone(),
    });

    let gate = Arc::new(tokio::sync::Notify::new());
    *prov.wake_gate.lock() = Some(gate.clone());
    let start_wake = || ctrl.wake(&created.machine_id, prov.as_ref(), peer.as_ref(), 60_000);
    let mut leader = std::pin::pin!(start_wake());
    let mut follower = std::pin::pin!(start_wake());
    std::future::poll_fn(|cx| {
        assert!(leader.as_mut().poll(cx).is_pending());
        assert!(follower.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    gate.notify_one();

    let (leader, follower) = tokio::join!(leader, follower);
    assert_eq!(leader.unwrap_err().code(), "wake_deadline_exceeded");
    assert_eq!(follower.unwrap_err().code(), "wake_deadline_exceeded");
    assert_eq!(prov.wake_calls.load(Ordering::SeqCst), 1);
    assert_eq!(prov.wake_billing_failures.load(Ordering::SeqCst), 1);
}
