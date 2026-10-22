use super::*;

#[tokio::test(start_paused = true)]
async fn capture_completes_on_its_event_without_waiting_for_the_deadline() {
    let (sender, mut receiver) = oneshot::channel();
    sender.send(Ok(Some("inventory".to_string()))).unwrap();
    let start = tokio::time::Instant::now();
    assert_eq!(
        receive(&mut receiver, CAPTURE_DEADLINE).await.unwrap(),
        Some("inventory".to_string())
    );
    assert_eq!(tokio::time::Instant::now(), start);
}

#[tokio::test(start_paused = true)]
async fn stuck_capture_returns_a_typed_timeout_and_retains_late_completion() {
    let (sender, mut receiver) = oneshot::channel::<Result<Option<String>, String>>();
    let error = receive(&mut receiver, CAPTURE_DEADLINE).await.unwrap_err();
    let error: serde_json::Value = serde_json::from_str(&error.message()).unwrap();
    assert_eq!(error["type"], "close_inventory_timeout");
    sender.send(Ok(Some("late-inventory".into()))).unwrap();
    assert_eq!(
        receiver.await.unwrap().unwrap().as_deref(),
        Some("late-inventory")
    );
}

#[tokio::test]
async fn exited_capture_returns_a_typed_failure() {
    let (sender, mut receiver) = oneshot::channel::<Result<(), String>>();
    drop(sender);
    let error = receive(&mut receiver, CAPTURE_DEADLINE).await.unwrap_err();
    let error: serde_json::Value = serde_json::from_str(&error.message()).unwrap();
    assert_eq!(error["type"], "close_inventory_failed");
}

#[test]
fn capture_claim_refuses_only_the_active_scope_and_releases_on_completion() {
    let workspace = uuid::Uuid::new_v4().to_string();
    let first = ScopeClaim::acquire(&workspace, Some("one")).unwrap();
    let error = ScopeClaim::acquire(&workspace, Some("one")).err().unwrap();
    let error: serde_json::Value = serde_json::from_str(&error).unwrap();
    assert_eq!(error["type"], "close_inventory_busy");
    let unrelated = ScopeClaim::acquire(&workspace, Some("two")).unwrap();
    drop(first);
    assert!(ScopeClaim::acquire(&workspace, Some("one")).is_ok());
    drop(unrelated);
}

#[tokio::test]
async fn capture_persists_inventory_before_return_and_settlement_releases_the_scope() {
    let mut mux = crate::multiplexer::state::MultiplexerState::empty();
    let workspace = mux.create_workspace("Capture fixture".into(), None).id;
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let inventory = fixture
        .service
        .capture_close_inventory(&workspace, None)
        .await
        .unwrap()
        .unwrap();
    let saved = fixture
        .service
        .mux
        .read()
        .closed_structure(&inventory)
        .unwrap()
        .unwrap();
    assert_eq!(saved.workspace.id, workspace);
    assert_eq!(
        saved.state,
        crate::multiplexer::state::StructureState::Captured
    );
    fixture
        .service
        .settle_close_inventory(Some(&inventory))
        .unwrap();
    assert!(fixture
        .service
        .mux
        .read()
        .closed_structure(&inventory)
        .unwrap()
        .is_none());
    assert!(fixture
        .service
        .capture_close_inventory(&workspace, None)
        .await
        .unwrap()
        .is_some());
}
