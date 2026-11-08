// WHAT: a closed agent tab's transcript over the three supported REST paths.
// WHY: `tab chat`, `child-history` and `sessions get` all lost a Worker once closed.
// ORACLE: the owner, parent and Team read it by id; anyone else learns nothing.
use super::*;
use crate::auth::auth_middleware;
use axum::{
    body::{to_bytes, Body},
    http::Request,
    routing::get,
    Router,
};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::tab_chat::chatbackend_history::{
    archive_path, ChatBackendHistory,
};
use codeterm_service_core::service::test_fixture::ServiceFixture;
use serde_json::Value;
use tower::ServiceExt;

const OWNER: &str = "owner-secret";
const BODY: &str = "opaque-closed-transcript-body";

struct Desk {
    _fixture: ServiceFixture,
    state: AppState,
    app: Router,
    worker: String,
    session: String,
    parent: String,
    teammate: String,
    stranger: String,
}

fn token(state: &AppState, tab: &str) -> String {
    state
        .svc
        .frontend_pty_env(Some(tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap()
}

fn close(state: &AppState, tab: &str) {
    let batch = {
        let mut mux = state.svc.mux.write();
        mux.close_tab(tab).unwrap();
        mux.take_closed_history_batch()
    };
    if let Some(batch) = batch {
        batch.persist();
    }
}

fn desk() -> Desk {
    let (mut mux, root) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&root).unwrap();
    let provider = format!("closed-acp-{}", uuid::Uuid::new_v4());
    let session = format!("session-{}", uuid::Uuid::new_v4());
    let parent = mux.create_tab(&workspace).unwrap();
    let teammate = mux.create_tab(&workspace).unwrap();
    let stranger = mux.create_tab(&workspace).unwrap();
    let worker = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&teammate).unwrap().team_id = Some("team-a".into());
    let tab = mux.get_tab_mut(&worker).unwrap();
    tab.agent_type = Some(provider.clone());
    tab.agent_session_id = Some(session.clone());
    tab.parent_tab_id = Some(parent.clone());
    tab.team_id = Some("team-a".into());
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some(OWNER.into());
    ChatBackendHistory::open_at(archive_path(&provider, &session))
        .try_upsert(&[codeterm_types::agent::chat_types::ChatMsg {
            id: "row-1".into(),
            kind: "assistant".into(),
            content: BODY.into(),
            ..Default::default()
        }])
        .unwrap();
    let app = Router::new()
        .route("/api/tabs/{id}/chat", get(crate::tab_chat_routes::snapshot))
        .route("/api/tabs/{id}/closed-children", get(closed_children))
        .route(
            "/api/sessions/{id}",
            get(crate::session_routes::get_session_api),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state.clone());
    Desk {
        _fixture: fixture,
        state,
        app,
        worker,
        session,
        parent,
        teammate,
        stranger,
    }
}

async fn get_json(app: &Router, path: &str, token: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_tab_reads_by_id_for_its_owner_parent_and_team_only() {
    let desk = desk();
    let chat = format!("/api/tabs/{}/chat", desk.worker);
    let parent = token(&desk.state, &desk.parent);
    let teammate = token(&desk.state, &desk.teammate);
    let stranger = token(&desk.state, &desk.stranger);
    close(&desk.state, &desk.worker);
    for caller in [OWNER, parent.as_str(), teammate.as_str()] {
        let (status, body) = get_json(&desk.app, &chat, caller).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["state"], "closed");
        assert_eq!(body["closed"], true);
        assert_eq!(body["count"], 1);
        assert_eq!(body["messages"][0]["content"], BODY);
        assert_eq!(body["tab"]["tab_id"], desk.worker.as_str());
    }
    let (status, body) = get_json(&desk.app, &chat, &stranger).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "closed_tab_read_refused");
    assert!(!body.to_string().contains(BODY));
    assert!(!body.to_string().contains(&desk.session));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_tab_stays_not_found_and_a_live_tab_never_reads_as_closed() {
    let desk = desk();
    let (status, body) = get_json(&desk.app, "/api/tabs/no-such-tab/chat", OWNER).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "chat_snapshot_refused");
    let chat = format!("/api/tabs/{}/chat", desk.parent);
    let (status, body) = get_json(&desk.app, &chat, OWNER).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.get("closed").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_parent_sees_its_closed_children_and_reads_them() {
    let desk = desk();
    let list = format!("/api/tabs/{}/closed-children", desk.parent);
    let parent = token(&desk.state, &desk.parent);
    let stranger = token(&desk.state, &desk.stranger);
    let (status, body) = get_json(&desk.app, &list, &parent).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 0);
    close(&desk.state, &desk.worker);
    let (status, body) = get_json(&desk.app, &list, &parent).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["children"][0]["tab_id"], desk.worker.as_str());
    let one = format!("{list}?child={}", desk.worker);
    let (status, body) = get_json(&desk.app, &one, &parent).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["messages"][0]["content"], BODY);
    for path in [&list, &one] {
        let (status, body) = get_json(&desk.app, path, &stranger).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!body.to_string().contains(&desk.worker));
    }
    let foreign = format!(
        "/api/tabs/{}/closed-children?child={}",
        desk.teammate, desk.worker
    );
    let (status, _) = get_json(&desk.app, &foreign, OWNER).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_get_covers_a_closed_runtime_tab_without_widening_authority() {
    let desk = desk();
    let stranger = token(&desk.state, &desk.stranger);
    let parent = token(&desk.state, &desk.parent);
    let by_session = format!("/api/sessions/{}", desk.session);
    let by_tab = format!("/api/sessions/{}", desk.worker);
    let (status, _) = get_json(&desk.app, &by_session, OWNER).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    close(&desk.state, &desk.worker);
    for (path, caller) in [
        (&by_session, OWNER),
        (&by_tab, OWNER),
        (&by_tab, parent.as_str()),
    ] {
        let (status, body) = get_json(&desk.app, path, caller).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["closed_tab"]["tab_id"], desk.worker.as_str());
        assert_eq!(body["session_id"], desk.session.as_str());
        assert_eq!(
            body["transcript_command"],
            format!("domios tab chat --tab {}", desk.worker)
        );
    }
    let (status, _) = get_json(&desk.app, &by_session, &stranger).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[test]
fn mesh_and_remote_callers_never_read_a_closed_tab() {
    let record = ClosedTranscript {
        tab_id: "worker".into(),
        provider: "provider".into(),
        session_id: "session".into(),
        cwd: String::new(),
        chat_backend: false,
        parent_tab_id: Some("parent".into()),
        team_id: Some("team".into()),
        title: String::new(),
        role: None,
        model: None,
        closed_at: 0,
        archives: Vec::new(),
    };
    assert!(may_read(&Reader::Owner, &record));
    assert!(!may_read(&Reader::Other, &record));
    assert!(!may_list(&Reader::Other, "parent", Some("team")));
    let agent = |tab: &str, team: Option<&str>| Reader::Agent {
        tab_id: tab.into(),
        team_id: team.map(str::to_string),
    };
    assert!(may_list(&agent("parent", None), "parent", None));
    assert!(may_list(
        &agent("mate", Some("team")),
        "parent",
        Some("team")
    ));
    assert!(!may_list(&agent("other", None), "parent", None));
    assert!(!may_list(&agent("other", Some("")), "parent", Some("")));
}
