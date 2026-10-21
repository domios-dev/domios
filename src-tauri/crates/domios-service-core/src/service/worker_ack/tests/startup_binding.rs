use super::*;
#[test]
fn startup_ack_accepts_a_session_that_bound_after_supervision_armed() {
    let registry = WorkerAckRegistry::default();
    let authority = StartupAuthority::new("parent", "child", "pty-a").for_binding(None, "nonce-a");
    let waiter = startup_receiver(&registry, authority);

    assert_eq!(
        registry.acknowledge_startup_for_binding(
            "child",
            "pty-a",
            Some("session-late"),
            Some("nonce-b")
        ),
        None
    );
    assert_eq!(
        registry.acknowledge_startup_for_binding(
            "child",
            "pty-a",
            Some("session-late"),
            Some("nonce-a")
        ),
        Some(AgentStartupAcknowledgement::Acknowledged)
    );
    assert_eq!(waiter.recv().unwrap(), AgentStartupSignal::Ready);
    assert_eq!(
        registry.acknowledge_startup_for_binding(
            "child",
            "pty-a",
            Some("session-late"),
            Some("nonce-a")
        ),
        Some(AgentStartupAcknowledgement::AlreadyAcknowledged)
    );
}
