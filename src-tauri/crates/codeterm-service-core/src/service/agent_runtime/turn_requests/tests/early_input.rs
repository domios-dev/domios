use super::*;

#[test]
fn app_server_turns_keep_one_native_user_row_per_turn() {
    let root = crate::test_support::fixture_manifest_dir().join("../../../plugins");
    let mut recorded = None;
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry
            .unwrap()
            .path()
            .join("tests/fixtures/runtime/multi-turn.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let case: Value = serde_json::from_str(&text).unwrap();
        if case["rpc"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["action"]["call"]["method"] == "turn/start")
        {
            assert!(
                recorded.is_none(),
                "The native app-server fixture must be unambiguous"
            );
            recorded = Some(case);
        }
    }
    let fixture_json = recorded.expect("A native app-server multi-turn fixture must exist");
    let (fixture, tab, _peer, _cleanup) = engine::test_support::runtime_fixture();
    let session = engine::for_tab(&tab).unwrap();
    let claims = serde_json::to_string(&fixture_json["context"]["claimedSends"])
        .unwrap()
        .replace("thread-fixture", "own-thread");
    session.context.lock().claimed_sends = serde_json::from_str(&claims).unwrap();
    let events = serde_json::to_string(&fixture_json["expected"]["events"])
        .unwrap()
        .replace("thread-fixture", "own-thread");
    let events: Vec<AgentRuntimeEvent> = serde_json::from_str(&events).unwrap();
    let epoch = session.context.lock().epoch;
    for _ in 0..2 {
        crate::async_runtime::block_on(subscriber::publish_events(
            &fixture.service,
            &session,
            epoch,
            events.clone(),
            10,
        ))
        .unwrap();
        let rows = session.cache.lock().snapshot();
        let users: Vec<_> = rows.iter().filter(|row| row.kind == "user").collect();
        assert_eq!(
            users.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            vec!["user-a", "user-b"]
        );
        assert!(users.iter().all(|row| row.runtime_source_item_id.is_none()));
    }
}
