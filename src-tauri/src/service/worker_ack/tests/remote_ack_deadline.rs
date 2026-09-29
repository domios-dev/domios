use super::*;

#[test]
fn remote_ack_before_deadline_disarms_it() {
    let registry = WorkerAckRegistry::default();
    let identity = WorkerAckIdentity {
        plan_id: "plan".into(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        tab_id: "remote:peer:tab".into(),
        startup_attempt_id: None,
        session_id: None,
        binding_nonce: None,
    };
    let arm = registry.arm(identity.clone());
    assert!(registry.acknowledge(&identity));
    assert!(super::super::await_remote_worker_ack(
        &arm.receiver.unwrap(),
        std::time::Duration::ZERO
    ));
}

#[test]
fn remote_ack_deadline_expires_without_an_ack_event() {
    let (_sender, receiver) = std::sync::mpsc::channel();
    assert!(!super::super::await_remote_worker_ack(
        &receiver,
        std::time::Duration::ZERO
    ));
}
