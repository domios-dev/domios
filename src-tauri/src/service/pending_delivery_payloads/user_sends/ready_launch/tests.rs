use super::*;

fn launch() -> UserSendAuthority {
    UserSendAuthority {
        tab_id: "tab".into(),
        pty_id: Some("pty".into()),
        binding_nonce: "launch".into(),
        pty_session_generation: Some(1),
        provider_id: "provider".into(),
        session_id: None,
        observed_session_id: None,
        prelaunch: false,
    }
}

#[test]
fn ready_once_ignores_session_detection_and_survives_busy_unknown_chat_and_supporter_changes() {
    let mut latch = ReadyLaunchLatch::default();
    let mut authority = launch();
    assert!(!latch.is_ready(&authority));
    assert!(latch.mark_ready(&authority));
    for _event in [
        "busy",
        "unknown",
        "idle-stuck",
        "chat-detach",
        "chat-reattach",
        "supporter-outage",
    ] {
        authority.observed_session_id = Some("session-detected-later".into());
        assert!(latch.is_ready(&authority));
        authority.session_id = authority.observed_session_id.take();
        assert!(latch.is_ready(&authority));
    }
}

#[test]
fn new_launch_or_process_generation_requires_its_own_first_ready_event() {
    let mut latch = ReadyLaunchLatch::default();
    let authority = launch();
    latch.mark_ready(&authority);
    let mut next = authority.clone();
    next.binding_nonce = "next-launch".into();
    assert!(!latch.is_ready(&next));
    assert!(latch.mark_ready(&next));
    assert!(!latch.is_ready(&authority));
    next.pty_session_generation = Some(2);
    assert!(!latch.is_ready(&next));
    latch.mark_ready(&next);
    assert!(latch.is_ready(&next));
    next.pty_id = None;
    assert!(!latch.is_ready(&next));
    assert!(!latch.mark_ready(&next));
}

#[test]
fn only_pre_ready_admissions_wait_for_earlier_preparation() {
    assert!(admission_order_blocks(true, true));
    assert!(!admission_order_blocks(false, true));
    assert!(!admission_order_blocks(true, false));
}
