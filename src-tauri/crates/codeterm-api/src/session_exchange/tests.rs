use super::capture_snapshot;
use super::import_target_allowed;
use codeterm_service_core::session::store::SessionSnapshot;
use codeterm_types::agent::chat_types::ChatMsg;
use serde_json::json;

fn fixture_document() -> serde_json::Value {
    json!({"format":"upgrade-restore-v1","sanitized":true,"plans":[],
        "snapshot":{"version":"3","timestamp":1,"active_workspace_id":"ws",
            "workspaces":[{"id":"ws","name":"Fixture","active_tab_id":"tab",
                "tabs":[{"id":"tab","title":"Fixture","split_tree":{"type":"Leaf","tab_id":"tab",
                    "cwd":"/sanitized","agent_type":"fixture-provider","agent_session_id":"history-session",
                    "agent_identity":{"session_binding_nonce":"launch-nonce","reported_session_binding_nonce":"launch-nonce",
                        "restorable_launch":{"identity_instructions":"opaque-identity"}}}}]}]}})
}

#[test]
fn fixture_document_refuses_malformed_and_unpublishable_snapshots() {
    let valid = fixture_document();
    assert!(super::fixture_document(&valid).is_ok());
    let mut unsigned = valid.clone();
    unsigned["sanitized"] = json!(false);
    let mut wrong_format = valid.clone();
    wrong_format["format"] = json!("other");
    let mut missing_tab = valid.clone();
    missing_tab["snapshot"]["workspaces"][0]["active_tab_id"] = json!("missing");
    let mut orphan_team = valid.clone();
    orphan_team["snapshot"]["teams"] =
        json!([{"id":"team","workspace_id":"missing","created_at":1,"lifecycle":"active"}]);
    let mut malformed = valid.clone();
    malformed["plans"] = json!([{"id":"not-a-plan"}]);
    let mut mismatched_leaf = valid.clone();
    mismatched_leaf["snapshot"]["workspaces"][0]["tabs"][0]["split_tree"]["tab_id"] =
        json!("different");
    let mut incomplete_backend = valid.clone();
    incomplete_backend["snapshot"]["workspaces"][0]["tabs"][0]["split_tree"]
        ["chatbackend_plugin_id"] = json!("fixture-backend");
    let mut empty = valid.clone();
    empty["snapshot"]["workspaces"] = json!([]);
    empty["snapshot"]["active_workspace_id"] = serde_json::Value::Null;
    let mut split = valid.clone();
    let leaf = split["snapshot"]["workspaces"][0]["tabs"][0]["split_tree"].clone();
    split["snapshot"]["workspaces"][0]["tabs"][0]["split_tree"] = json!({"type":"Split","orientation":"Horizontal","ratio":0.5,"children":[leaf.clone(),leaf]});
    for document in [
        unsigned,
        wrong_format,
        missing_tab,
        orphan_team,
        malformed,
        mismatched_leaf,
        incomplete_backend,
        empty,
        split,
    ] {
        assert!(super::fixture_document(&document).is_err());
    }
}

#[test]
fn fixture_cwd_replay_preserves_history_and_launch_binding_identities() {
    let (mut snapshot, _) = super::fixture_document(&fixture_document()).unwrap();
    let leaf = &mut snapshot.workspaces[0].tabs[0].split_tree;
    super::replay_cwd(leaf, "/fixture/project");
    let value = serde_json::to_value(leaf).unwrap();
    assert_eq!(value["cwd"], "/fixture/project");
    assert_eq!(value["tab_id"], "tab");
    assert_eq!(value["agent_session_id"], "history-session");
    assert_eq!(
        value["agent_identity"]["session_binding_nonce"],
        "launch-nonce"
    );
    assert_eq!(
        value["agent_identity"]["reported_session_binding_nonce"],
        "launch-nonce"
    );
    assert_eq!(
        value["agent_identity"]["restorable_launch"]["identity_instructions"],
        "opaque-identity"
    );
}

#[tokio::test]
async fn fixture_import_auth_middleware_refuses_missing_and_wrong_tokens_without_mutation() {
    use axum::{body::Body, http::Request, routing::post, Router};
    use codeterm_service_core::{
        multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
    };
    use tower::ServiceExt;
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let state = crate::transport_tabs::fixture::app_state(&fixture);
    let before = serde_json::to_value(fixture.service.mux.read().to_session_snapshot()).unwrap();
    let app = Router::new()
        .route("/api/session/backups/import", post(super::import))
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state,
            crate::auth::auth_middleware,
        ));
    for token in ["", "invalid-fixture-token"] {
        let request = Request::builder()
            .method("POST")
            .uri("/api/session/backups/import")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from(fixture_document().to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
    }
    let mut after = serde_json::to_value(fixture.service.mux.read().to_session_snapshot()).unwrap();
    after["timestamp"] = before["timestamp"].clone();
    assert_eq!(after, before);
}

#[test]
fn populated_snapshot_exports_unavailable_leaf_and_remaining_histories() {
    let leaf = |id: &str, extra: serde_json::Value| {
        let mut value = json!({"type":"Leaf","tab_id":id,"cwd":"/private/project",
            "agent_type":"fixture-provider","agent_session_id":format!("session-{id}")});
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        value
    };
    let snapshot: SessionSnapshot = serde_json::from_value(json!({
        "version":"1","timestamp":1,"active_workspace_id":"workspace",
        "workspaces":[{"id":"workspace","name":"private desk","active_tab_id":"group",
            "tabs":[{"id":"group","title":"private group","split_tree":{
                "type":"Split","orientation":"Horizontal","ratio":0.5,"children":[
                    leaf("unavailable", json!({"chatbackend_plugin_id":"fixture-provider",
                        "chatbackend_session_id":"session-unavailable"})),
                    leaf("readable", json!({})),
                    leaf("empty", json!({})),
                    leaf("shell", json!({"agent_type":null,"agent_session_id":null}))
                ]}}]}]
    }))
    .unwrap();
    let mut visited = Vec::new();
    let (safe, histories, measurements) = capture_snapshot(&snapshot, |node| {
        let (_, session) = node.agent_session().unwrap();
        visited.push(session.to_owned());
        match session {
            "session-unavailable" => Err("private transcript path".into()),
            "session-readable" => Ok(vec![ChatMsg {
                id: "message-readable".into(),
                kind: "assistant".into(),
                content: "private message".into(),
                tool_flow_id: Some("turn-readable".into()),
                ..Default::default()
            }]),
            "session-empty" => Ok(Vec::new()),
            _ => panic!("Non-agent leaf must not request a transcript"),
        }
    })
    .unwrap();
    let restored: SessionSnapshot = serde_json::from_value(safe.clone()).unwrap();
    assert_eq!(restored.workspaces[0].tabs.len(), 1);
    assert_eq!(
        safe["workspaces"][0]["tabs"][0]["split_tree"]["children"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        visited,
        ["session-unavailable", "session-readable", "session-empty"]
    );
    assert_eq!(histories.as_array().unwrap().len(), 4);
    assert_eq!(histories[0]["tab_id"], "unavailable");
    assert_eq!(histories[0]["transcript"]["status"], "unavailable");
    assert_eq!(histories[0]["transcript"]["backing_kind"], "chat_backend");
    assert_eq!(histories[0]["transcript"]["provider"], "fixture-provider");
    assert_eq!(histories[0]["messages"], json!([]));
    assert_eq!(histories[1]["transcript"]["status"], "captured");
    assert_eq!(histories[1]["messages"][0]["id"], "message-readable");
    assert_eq!(
        histories[1]["messages"][0]["content"],
        "x".repeat("private message".len())
    );
    assert_eq!(histories[2]["transcript"]["status"], "captured");
    assert_eq!(histories[2]["messages"], json!([]));
    assert_eq!(histories[3]["transcript"]["status"], "not_applicable");
    assert_eq!(histories[3]["transcript"]["backing_kind"], "pty");
    assert_eq!(measurements.len(), 4);
    assert_eq!(measurements[1]["history_items"], 1);
    assert_eq!(
        measurements[1]["history_bytes"],
        serde_json::to_vec(&histories[1]["messages"]).unwrap().len()
    );
    assert!(!safe.to_string().contains("private"));
    assert!(!histories.to_string().contains("private"));
}

#[test]
fn fixture_import_cannot_reach_production_or_ordinary_dev() {
    assert!(!import_target_allowed(
        false,
        7686,
        "-upgrade-restore-fixture"
    ));
    assert!(!import_target_allowed(
        true,
        7685,
        "-upgrade-restore-fixture"
    ));
    assert!(!import_target_allowed(true, 7686, ""));
    assert!(!import_target_allowed(true, 7686, "-another-team"));
    assert!(import_target_allowed(
        true,
        17686,
        "-upgrade-restore-fixture"
    ));
}

#[test]
fn exported_plans_streams_every_plan_without_filling_the_cache() {
    use codeterm_service_core::service::plan::load_audit::{execution_plan, seed, start};
    let dir = tempfile::tempdir().unwrap();
    let store = codeterm_service_core::service::plan::PlanStore::new_for_test(dir.path());
    let first = seed(&store, execution_plan(dir.path(), "first", "owner", vec![]));
    let second = seed(
        &store,
        execution_plan(dir.path(), "second", "owner", vec![]),
    );
    start(&store);

    let exported = super::exported_plans(&store).unwrap();
    let mut ids: Vec<String> = exported
        .as_array()
        .unwrap()
        .iter()
        .map(|plan| plan["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    let mut expected = vec![first, second];
    expected.sort();
    assert_eq!(ids, expected);
    assert!(store.plan_loads_for_test().is_empty());
}

#[test]
fn exported_plans_with_tasks_attempts_and_agent_states_deserialize_for_import() {
    use codeterm_service_core::service::plan::load_audit::{attempt, execution_plan, start, task};
    use codeterm_types::plan::{CuratorAgentState, Plan, TaskStatus};

    let dir = tempfile::tempdir().unwrap();
    let store = codeterm_service_core::service::plan::PlanStore::new_for_test(dir.path());
    let mut worker = task("task-bound", TaskStatus::InProgress, Some("worker-tab"));
    worker.name = "private task".into();
    worker.execution_attempts = vec![attempt("attempt-bound", "worker-tab")];
    let mut plan = execution_plan(dir.path(), "private plan", "owner-tab", vec![worker]);
    for name in ["private-agent-a", "private-agent-b"] {
        plan.control_loop.agent_states.insert(
            name.into(),
            CuratorAgentState {
                state: "private state".into(),
                since: 12345,
            },
        );
    }
    plan.control_loop
        .task_states
        .insert("private-task".into(), "private status".into());
    plan.elements = serde_json::from_value(json!([{
        "id":"element-bound", "kind":"private kind",
        "content":{"state":"private content", "since":98765}
    }]))
    .unwrap();
    store.save(&mut plan, 1).unwrap();
    start(&store);

    let exported = super::exported_plans(&store).unwrap();
    let restored: Vec<Plan> = serde_json::from_value(exported.clone()).unwrap();
    assert_eq!(restored.len(), 1);
    let restored = &restored[0];
    assert_eq!(restored.id, plan.id);
    assert_eq!(restored.tasks[0].id, "task-bound");
    assert_eq!(restored.tasks[0].execution_attempts[0].id, "attempt-bound");
    assert_eq!(restored.control_loop.agent_states.len(), 2);
    for (key, state) in &restored.control_loop.agent_states {
        assert!(!key.contains("private"));
        assert_eq!(state.state, super::sanitize::filler("private state"));
        assert_eq!(state.since, 12345);
    }
    assert!(!exported.to_string().contains("private"));
    assert!(!exported.to_string().contains("98765"));
    assert_eq!(
        serde_json::to_vec(&exported[0]).unwrap().len(),
        serde_json::to_vec(&plan).unwrap().len()
    );
    let mut document = fixture_document();
    document["plans"] = exported;
    let (_, imported) = super::fixture_document(&document).unwrap();
    assert_eq!(serde_json::to_value(imported).unwrap(), document["plans"]);
    assert!(store.plan_loads_for_test().is_empty());
}
