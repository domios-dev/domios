use super::*;
use axum::routing::post;
use codeterm_service_core::agent::test_provider::MockProvider;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn session_start_keeps_health_list_and_send_responsive_while_journal_writer_is_held() {
    let (mut mux, tab) = MultiplexerState::new();
    let target = mux.get_tab_mut(&tab).unwrap();
    target.role = None;
    target.agent_type = Some("hook-journal-fixture".into());
    target.pty_id = Some("own-pty".into());
    target.session_binding_nonce = Some("own-nonce".into());
    target.agent_booting = true;
    target.boot_stage = Some(codeterm_types::pty::BootStage::ProviderReady);
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::with_session("hook-journal-fixture", "own-session").with_hooks(),
    ));
    let state = app_state(&fixture);
    let journal = fixture.service.lifecycle_journal.clone();
    let held = HeldDependency::new(move |locked, release| {
        journal.with_writer_held_for_test(|| {
            locked.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(5)).unwrap();
        });
    })
    .await;
    let (entered, observed) = tokio::sync::oneshot::channel();
    let entered = Arc::new(parking_lot::Mutex::new(Some(entered)));
    fixture
        .ui
        .listen("session-binding-state-changed", move |_| {
            if let Some(entered) = entered.lock().take() {
                let _ = entered.send(());
            }
        });
    let request = axum::http::Request::builder().method("POST").uri(format!("/hook/{tab}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::json!({"session_id":"own-session", "provider_id":"hook-journal-fixture", "binding_nonce":"own-nonce"}).to_string())).unwrap();
    let router = Router::new()
        .route("/hook/{id}", post(crate::hook_routes::hook_session_start))
        .with_state(state.clone());
    let operation = tokio::spawn(router.oneshot(request));
    tokio::time::timeout(Duration::from_secs(5), observed)
        .await
        .unwrap()
        .unwrap();
    assert!(!operation.is_finished());
    crate::rest_liveness_tests::assert_health_list_send_respond(state).await;
    assert!(!operation.is_finished());
    drop(held);
    let response = operation.await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let records = fixture.service.lifecycle_journal.load().records;
    assert!(records.iter().any(|record| matches!(record,
        codeterm_service_core::session::lifecycle::JournalRecord::Transition { entry }
        if matches!(&entry.transition, codeterm_service_core::session::lifecycle::LifecycleTransition::SessionBound { session_id, .. } if session_id == "own-session")
    )));
}
