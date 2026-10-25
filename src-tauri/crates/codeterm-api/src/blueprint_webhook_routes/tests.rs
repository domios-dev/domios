use super::*;
use axum::{body::to_bytes, http::Request, Router};
use codeterm_sentinel_host::{
    scheduler::{fixture_handle_with_ui, Handle},
    store::{Owner, Store},
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use codeterm_storage::{Storage, StorageConfig};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;

struct Fixture {
    root: tempfile::TempDir,
    fixture: ServiceFixture,
    storage: Storage,
    handle: Handle,
    router: Router,
    id: String,
    creator: String,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let (mut mux, creator) = MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&creator).unwrap();
        mux.get_tab_mut(&creator).unwrap().agent_type = Some("fixture-agent".into());
        mux.get_tab_mut(&creator).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
        let fixture = ServiceFixture::new(mux);
        let storage = Storage::open(&root.path().join("store"), StorageConfig::default()).unwrap();
        let store = Store::with_storage(
            &storage,
            codeterm_service_core::cron::clock::MockClock::starting_at(1000),
        );
        let sink = root.path().join("payload.json");
        #[cfg(windows)]
        let command = format!(
            "[IO.File]::WriteAllText('{}',[Console]::In.ReadToEnd())",
            sink.display()
        );
        #[cfg(unix)]
        let command = format!("cat > '{}'", sink.display());
        let document = json!({"schema_version":2,"kind":"pipeline","name":"Webhook fixture","owner":"tab",
            "trigger":{"type":"webhook","schema":{"type":"object"}},
            "steps":[{"id":"capture","run":{"lit":command},"in":{"type":"object"}}],"result":{"to":"notify"}});
        let job = store
            .prepare_document(
                document,
                Owner {
                    creator_tab_id: creator.clone(),
                    team_id: None,
                    workspace_id: workspace,
                    cwd: root.path().to_string_lossy().into_owned(),
                },
                true,
            )
            .unwrap();
        let handle = fixture_handle_with_ui(store, fixture.service.ui.clone());
        let id = job.id.clone();
        codeterm_sentinel_host::storage::commit(&handle.store, |store| store.stage_insert(job))
            .unwrap();
        codeterm_sentinel_host::manage_fixture_ports(&fixture.service, &handle);
        let router =
            crate::routes::create_router(crate::transport_tabs::fixture::app_state(&fixture));
        Self {
            root,
            fixture,
            storage,
            handle,
            router,
            id,
            creator,
        }
    }

    async fn configure(&self, action: &str, reveal: bool) -> (StatusCode, Value) {
        self.post(
            &format!("/api/blueprints/{}/webhook/configure", self.id),
            Some("preview-test-token"),
            &json!({
                "caller_tab_id":self.creator,"action":action,"reveal":reveal
            })
            .to_string(),
            None,
        )
        .await
    }

    async fn post(
        &self,
        path: &str,
        bearer: Option<&str>,
        body: &str,
        delivery: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json");
        if let Some(bearer) = bearer {
            request = request.header("authorization", format!("Bearer {bearer}"));
        }
        if let Some(delivery) = delivery {
            request = request.header("x-delivery-id", delivery);
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::from(body.to_owned())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 16384).await.unwrap();
        let value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"error":String::from_utf8_lossy(&bytes)}));
        (status, value)
    }

    fn close(self) {
        drop(self.router);
        drop(self.fixture);
        drop(self.handle);
        self.storage
            .shutdown()
            .wait(Duration::from_secs(10))
            .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_post_persists_one_run_and_keeps_shell_metacharacters_as_data() {
    let fixture = Fixture::new();
    let (status, enabled) = fixture.configure("enable", true).await;
    assert_eq!(status, StatusCode::OK);
    let secret = enabled["secret"].as_str().unwrap();
    let path = format!("/api/blueprints/{}/webhook", fixture.id);
    for bearer in [None, Some("wrong"), Some("preview-test-token")] {
        assert_eq!(
            fixture.post(&path, bearer, "{}", None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let marker = fixture.root.path().join("injected");
    let payload = json!({"command":format!("$(echo owned > '{}'); & | `whoami`", marker.display()),"path":"../../escape","executable":"malicious"});
    let (sent, done) = tokio::sync::oneshot::channel();
    let sent = Arc::new(parking_lot::Mutex::new(Some(sent)));
    let handle = fixture.handle.clone();
    let id = fixture.id.clone();
    fixture
        .fixture
        .ui
        .listen("automation-graph-changed", move |_| {
            if sent.lock().is_none() {
                return;
            }
            let read = handle.store.lock().typed_pipeline_log(&id, 100).unwrap();
            let rows = read.wait(Duration::from_secs(10)).unwrap().value;
            if rows.iter().any(|row| {
                row.step_name == "$run"
                    && row.outcome
                        == Some(codeterm_blueprint_types::blueprint_journal::JournalOutcome::Done)
            }) {
                if let Some(sent) = sent.lock().take() {
                    let _ = sent.send(rows);
                }
            }
        });
    let (status, accepted) = fixture
        .post(
            &path,
            Some(secret),
            &payload.to_string(),
            Some("delivery-1"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    let rows = tokio::time::timeout(Duration::from_secs(10), done)
        .await
        .unwrap()
        .unwrap();
    let (status, duplicate) = fixture
        .post(
            &path,
            Some(secret),
            &payload.to_string(),
            Some("delivery-1"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(duplicate["duplicate"], true);
    assert_eq!(duplicate["run_id"], accepted["run_id"]);
    let captured: Value =
        serde_json::from_slice(&std::fs::read(fixture.root.path().join("payload.json")).unwrap())
            .unwrap();
    assert_eq!(captured["value"], payload);
    assert!(!marker.exists());
    assert_eq!(
        rows.iter().filter(|row| row.step_name == "capture").count(),
        1
    );
    assert!(rows
        .iter()
        .all(|row| row.run_id == accepted["run_id"].as_str().unwrap()));
    let audit = rows.iter().find(|row| row.step_name == "$run").unwrap();
    assert!(!serde_json::to_string(&rows).unwrap().contains(secret));
    assert!(audit.reason.as_ref().is_some());
    fixture.close();
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_lifecycle_limits_and_inactive_graphs_refuse_without_secret_disclosure() {
    let fixture = Fixture::new();
    let (_, hidden) = fixture.configure("enable", false).await;
    assert!(hidden.get("secret").is_none());
    assert!(fixture.configure("enable", true).await.0.is_client_error());
    let (_, rotated) = fixture.configure("rotate", true).await;
    let old = rotated["secret"].as_str().unwrap().to_owned();
    let (_, current) = fixture.configure("rotate", true).await;
    let secret = current["secret"].as_str().unwrap();
    let path = format!("/api/blueprints/{}/webhook", fixture.id);
    assert_eq!(
        fixture.post(&path, Some(&old), "{}", None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        fixture.post(&path, Some(secret), "not JSON", None).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        fixture.post(&path, Some(secret), "7", None).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        fixture
            .post(
                &path,
                Some(secret),
                &"x".repeat(blueprint_webhook::MAX_BODY_BYTES + 1),
                None
            )
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let (_, url) = fixture.configure("show-url", false).await;
    assert!(url.get("secret").is_none());
    assert!(!url.to_string().contains(secret));
    assert_eq!(fixture.configure("disable", false).await.0, StatusCode::OK);
    let disabled = fixture.post(&path, Some(secret), "{}", None).await;
    let missing = fixture
        .post("/api/blueprints/missing/webhook", Some(secret), "{}", None)
        .await;
    assert_eq!(disabled, missing);
    assert_eq!(disabled.0, StatusCode::NOT_FOUND);
    let (_, enabled) = fixture.configure("enable", true).await;
    let mut job = fixture
        .handle
        .store
        .lock()
        .get(&fixture.id)
        .unwrap()
        .clone();
    job.machine.paused = true;
    codeterm_sentinel_host::storage::commit(&fixture.handle.store, |store| {
        store.stage_finish(&job)
    })
    .unwrap();
    assert_eq!(
        fixture
            .post(&path, enabled["secret"].as_str(), "{}", None)
            .await,
        missing
    );
    let hooks = blueprint_webhook::service(&fixture.fixture.service).unwrap();
    let now = chrono::Utc::now().timestamp_millis() as u64;
    for _ in 0..120 {
        let _ = hooks.before_read(now);
    }
    assert_eq!(
        fixture.post(&path, Some(secret), "{}", None).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    fixture.close();
}

#[test]
fn webhook_auth_exception_matches_only_the_exact_post_ingress() {
    use axum::http::Method;
    assert!(crate::auth::is_custom_auth(
        &Method::POST,
        "/api/blueprints/graph/webhook"
    ));
    for path in [
        "/api/blueprints//webhook",
        "/api/blueprints/a/b/webhook",
        "/api/blueprints/a/webhook/configure",
    ] {
        assert!(!is_ingress(path));
        assert!(!crate::auth::is_custom_auth(&Method::POST, path));
    }
    assert!(!crate::auth::is_custom_auth(
        &Method::GET,
        "/api/blueprints/graph/webhook"
    ));
}
