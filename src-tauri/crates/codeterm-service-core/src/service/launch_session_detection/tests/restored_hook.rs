use super::*;
use crate::agent::test_provider::MockProvider;
use crate::service::chat_send_transaction::ChatSendState;
use crate::service::hook_session_binding::{
    acknowledge_and_reconcile_session_binding, bind_reported_session,
    unchanged_acknowledged_hook_report,
};
use crate::service::parked_user_delivery::test_support::{
    attach_fixture_pty, await_owner_idle, pre_ready_fixture, UserSubmitBoundary,
};
use std::sync::{mpsc, Arc};

#[test]
fn restored_first_send_waits_for_hook_and_session_start_duplicate_never_replays() {
    let (fixture, tab) = pre_ready_fixture();
    {
        let mut mux = fixture.service.mux.write();
        let restored = mux.get_tab_mut(&tab).unwrap();
        restored.restored_from_snapshot = true;
        restored.remember_resume_binding_nonce("opaque-session");
        assert_eq!(
            resumed_session_target(restored, Some("opaque-session")),
            Some("opaque-session")
        );
    }
    attach_fixture_pty(&fixture, &tab);
    let provider = MockProvider::with_session("opaque-provider", "opaque-session").with_hooks();
    assert!(!provider_owns_resume_session(
        &provider,
        "/work",
        "opaque-session"
    ));
    assert!(provider_owns_resume_session(
        &MockProvider::with_session("plain-provider", "opaque-session"),
        "/work",
        "opaque-session"
    ));
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));
    let (writes_tx, writes_rx) = mpsc::channel();
    let _boundary = UserSubmitBoundary::install(
        &tab,
        Arc::new(move |record, text| {
            writes_tx
                .send((record.send_id.clone(), text.to_owned()))
                .unwrap();
            Ok(())
        }),
    );
    let first = fixture
        .service
        .start_human_chat_send(
            &tab,
            crate::service::messenger::HumanInput::composer("opaque-first-turn"),
            None,
            None,
        )
        .unwrap();
    await_owner_idle(&fixture, &tab);
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&first.send_id)
            .unwrap()
            .state,
        ChatSendState::Pending
    );
    assert!(matches!(
        writes_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    let claim = bind_reported_session(
        &fixture.service.mux,
        fixture.service.agent_registry.as_ref(),
        &tab,
        "opaque-provider",
        "opaque-session",
        Some("opaque-current-nonce"),
    )
    .unwrap();
    acknowledge_and_reconcile_session_binding(
        &fixture.service,
        &tab,
        &claim,
        Some("opaque-current-nonce"),
    )
    .unwrap();
    fixture
        .service
        .emit_boot_stage(&tab, codeterm_types::pty::BootStage::AgentReady, None);
    await_owner_idle(&fixture, &tab);
    assert_eq!(
        writes_rx.try_recv().unwrap(),
        (first.send_id.clone(), "opaque-first-turn".into())
    );

    for _ in 0..2 {
        let duplicate = bind_reported_session(
            &fixture.service.mux,
            fixture.service.agent_registry.as_ref(),
            &tab,
            "opaque-provider",
            "opaque-session",
            Some("opaque-current-nonce"),
        )
        .unwrap();
        assert!(duplicate.is_unchanged_acknowledged_report());
        let response = unchanged_acknowledged_hook_report(
            &fixture.service,
            &tab,
            "opaque-provider",
            "opaque-session",
            "resume",
            &duplicate,
            Some("opaque-current-nonce"),
        );
        assert_eq!(
            response.0,
            serde_json::json!({"ok": true, "changed": false})
        );
        await_owner_idle(&fixture, &tab);
        assert!(matches!(
            writes_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }
    assert!(fixture
        .service
        .pending_delivery_payloads
        .user_sends
        .records(&tab)
        .unwrap()
        .is_empty());
    assert_eq!(
        fixture
            .service
            .chat_send_ledger
            .status(&first.send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
}
