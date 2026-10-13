use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::Manager;

use super::{
    auth::{caller_access_level, caller_peer_id},
    routes::AppState,
};
use crate::sentinel::graph::Snapshot;
use crate::service::sentinel::graph::explicit_team;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphQuery {
    workspace_id: String,
    team_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceGraphQuery {
    workspace_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopBody {
    workspace_id: String,
    team_id: Value,
    caller_tab_id: Option<String>,
    session_binding_nonce: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteBody {
    workspace_id: String,
    team_id: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventBody {
    plugin_id: String,
    event_id: String,
    payload: Value,
}

pub async fn event_ingress(
    State(state): State<AppState>,
    Path(subscription_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<EventBody>,
) -> Result<(StatusCode, Json<Value>), GraphError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| {
            failure((
                StatusCode::UNAUTHORIZED,
                "subscription bearer required".into(),
            ))
        })?;
    let payload_bytes = serde_json::to_vec(&body.payload)
        .map_err(|error| failure((StatusCode::BAD_REQUEST, error.to_string())))?
        .len();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default();
    let decision = state.svc.graph_event_subscriptions.admit(
        codeterm_plugin::graph_events::AdmissionRequest {
            subscription_id: &subscription_id,
            token,
            plugin_id: &body.plugin_id,
            event_id: &body.event_id,
            payload: &body.payload,
            payload_bytes,
            now_ms,
        },
    );
    match decision {
        codeterm_plugin::graph_events::AdmissionDecision::Admit => {
            if let Some(pipeline_id) = state
                .svc
                .graph_event_subscriptions
                .pipeline_id_for(&subscription_id)
            {
                if let Some(handle) = state
                    .svc
                    .app_handle
                    .try_state::<crate::sentinel::scheduler::Handle>()
                {
                    handle.notify_event_subscription_changed(&pipeline_id);
                }
            }
            Ok((
                StatusCode::ACCEPTED,
                Json(json!({"ok":true,"admitted":true})),
            ))
        }
        codeterm_plugin::graph_events::AdmissionDecision::Refuse(reason) => {
            let (status, code) = admission_refusal_status(reason);
            if let Some(pipeline_id) = state
                .svc
                .graph_event_subscriptions
                .pipeline_id_for(&subscription_id)
            {
                if let Some(handle) = state
                    .svc
                    .app_handle
                    .try_state::<crate::sentinel::scheduler::Handle>()
                {
                    handle.notify_event_subscription_changed(&pipeline_id);
                }
            }
            Err((status, Json(json!({"ok":false,"error":{"code":code}}))))
        }
    }
}

pub async fn snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<GraphQuery>,
) -> Result<Json<Snapshot>, GraphError> {
    caller_access_level(&state, &headers).map_err(failure)?;
    let team = query.team_id.ok_or_else(|| {
        failure((
            StatusCode::BAD_REQUEST,
            "team_id is required; empty means unteamed".into(),
        ))
    })?;
    let team = if team.is_empty() { None } else { Some(team) };
    let peer = caller_peer_id(&state, &headers).is_some();
    let mut snapshot = tokio::task::spawn_blocking(move || {
        state
            .svc
            .automation_graph_snapshot(&query.workspace_id, team.as_deref())
    })
    .await
    .map_err(|error| failure((StatusCode::INTERNAL_SERVER_ERROR, error.to_string())))?
    .map_err(|error| failure((StatusCode::BAD_REQUEST, error)))?;
    if peer {
        for node in &mut snapshot.nodes {
            if node["stop"]["allowed"] == true {
                node["stop"] =
                    json!({"allowed":false,"reason":"peer-origin automation stop is unavailable"});
            }
        }
    }
    Ok(Json(snapshot))
}

pub async fn workspace_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WorkspaceGraphQuery>,
) -> Result<Json<Snapshot>, GraphError> {
    caller_access_level(&state, &headers).map_err(failure)?;
    let snapshot = tokio::task::spawn_blocking(move || {
        state
            .svc
            .automation_graph_workspace_snapshot(&query.workspace_id)
    })
    .await
    .map_err(|error| failure((StatusCode::INTERNAL_SERVER_ERROR, error.to_string())))?
    .map_err(|error| failure((StatusCode::BAD_REQUEST, error)))?;
    Ok(Json(snapshot))
}

pub async fn stop(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<StopBody>,
) -> Result<Json<Value>, GraphError> {
    caller_access_level(&state, &headers).map_err(failure)?;
    if caller_peer_id(&state, &headers).is_some() {
        return Err(failure((
            StatusCode::FORBIDDEN,
            "peer-origin automation stop is unavailable".into(),
        )));
    }
    let team = explicit_team(&body.team_id).map_err(|e| failure((StatusCode::BAD_REQUEST, e)))?;
    let result = tokio::task::spawn_blocking(move || {
        if let Some(tab) = body.caller_tab_id {
            state.svc.automation_graph_stop_tab(
                &body.workspace_id,
                team.as_deref(),
                &id,
                &tab,
                body.session_binding_nonce.as_deref(),
            )
        } else {
            if body.session_binding_nonce.is_some() {
                return Err("session nonce requires caller_tab_id".into());
            }
            state
                .svc
                .automation_graph_stop_host(&body.workspace_id, team.as_deref(), &id)
        }
    })
    .await
    .map_err(|e| failure((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())))?
    .map_err(|e| failure((StatusCode::BAD_REQUEST, e)))?;
    Ok(Json(
        json!({"id":result.id,"state":"stopped","reason":result.last_reason}),
    ))
}

pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<DeleteBody>,
) -> Result<StatusCode, GraphError> {
    caller_access_level(&state, &headers).map_err(failure)?;
    if caller_peer_id(&state, &headers).is_some() {
        return Err(failure((
            StatusCode::FORBIDDEN,
            "peer-origin automation delete is unavailable".into(),
        )));
    }
    let team = explicit_team(&body.team_id).map_err(|e| failure((StatusCode::BAD_REQUEST, e)))?;
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .automation_graph_delete_host(&body.workspace_id, team.as_deref(), &id)
    })
    .await
    .map_err(|e| failure((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())))?
    .map_err(|e| failure((StatusCode::BAD_REQUEST, e)))?;
    Ok(StatusCode::NO_CONTENT)
}

type GraphError = (StatusCode, Json<Value>);

fn admission_refusal_status(
    reason: codeterm_plugin::graph_events::AdmitRefusal,
) -> (StatusCode, &'static str) {
    use codeterm_plugin::graph_events::AdmitRefusal;
    match reason {
        AdmitRefusal::UnknownSubscription => (StatusCode::NOT_FOUND, "unknown_subscription"),
        AdmitRefusal::BadToken => (StatusCode::UNAUTHORIZED, "bad_token"),
        AdmitRefusal::ForeignPlugin => (StatusCode::FORBIDDEN, "foreign_plugin"),
        AdmitRefusal::InvalidPayload => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_payload"),
        AdmitRefusal::Oversize => (StatusCode::PAYLOAD_TOO_LARGE, "oversize"),
        AdmitRefusal::Duplicate => (StatusCode::CONFLICT, "duplicate"),
        AdmitRefusal::Backpressure => (StatusCode::SERVICE_UNAVAILABLE, "backpressure"),
        AdmitRefusal::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
    }
}

fn failure((status, message): (StatusCode, String)) -> GraphError {
    let (status, code) = if message.starts_with("blueprint_store_unsupported_format:") {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "blueprint_store_unsupported_format",
        )
    } else if message.starts_with("blueprint_store_load_failed:") {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "blueprint_store_load_failed",
        )
    } else if message.starts_with("blueprint_store_persistence_failed:") {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "blueprint_store_persistence_failed",
        )
    } else if status == StatusCode::UNAUTHORIZED {
        (status, "unauthorized")
    } else {
        (status, "automation_refused")
    };
    (
        status,
        Json(
            json!({"ok":false,"error":{"code":code,"message":message.replace("Sentinels", "Blueprints").replace("Sentinel", "Blueprint").replace("sentinel ", "blueprint ")}}),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blueprint_event_ingress_refusal_statuses_cover_admission_table() {
        use codeterm_plugin::graph_events::AdmitRefusal::*;
        for (reason, status, code) in [
            (
                UnknownSubscription,
                StatusCode::NOT_FOUND,
                "unknown_subscription",
            ),
            (BadToken, StatusCode::UNAUTHORIZED, "bad_token"),
            (ForeignPlugin, StatusCode::FORBIDDEN, "foreign_plugin"),
            (
                InvalidPayload,
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_payload",
            ),
            (Oversize, StatusCode::PAYLOAD_TOO_LARGE, "oversize"),
            (Duplicate, StatusCode::CONFLICT, "duplicate"),
            (
                Backpressure,
                StatusCode::SERVICE_UNAVAILABLE,
                "backpressure",
            ),
            (RateLimited, StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
        ] {
            assert_eq!(admission_refusal_status(reason), (status, code));
        }
    }

    #[test]
    fn sentinel_graph_http_adapter_requires_explicit_team_and_preserves_tab_declaration() {
        let omitted: GraphQuery = serde_json::from_value(json!({"workspace_id":"w"})).unwrap();
        assert!(omitted.team_id.is_none());
        let null_team: GraphQuery =
            serde_json::from_value(json!({"workspace_id":"w","team_id":""})).unwrap();
        assert_eq!(null_team.team_id.as_deref(), Some(""));
        assert!(serde_json::from_value::<GraphQuery>(
            json!({"workspace_id":"w","team_id":"","scenario_id":"invented"})
        )
        .is_err());
        assert!(serde_json::from_value::<StopBody>(json!({"workspace_id":"w"})).is_err());
        let tab: StopBody=serde_json::from_value(json!({"workspace_id":"w","team_id":null,"caller_tab_id":"declared-tab","session_binding_nonce":"declared-nonce"})).unwrap();
        assert_eq!(tab.caller_tab_id.as_deref(), Some("declared-tab"));
        assert_eq!(tab.session_binding_nonce.as_deref(), Some("declared-nonce"));
        assert_eq!(explicit_team(&tab.team_id).unwrap(), None);
        let (status, Json(error)) = failure((
            StatusCode::BAD_REQUEST,
            "blueprint_store_unsupported_format: preserve definitions".into(),
        ));
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(error["error"]["code"], "blueprint_store_unsupported_format");
    }

    #[cfg(debug_assertions)]
    mod desktop {
        use super::*;
        use crate::service::test_fixture::ServiceFixture;
        use axum::{
            body::{to_bytes, Body},
            http::{Method, Request},
            Router,
        };
        use std::sync::Arc;
        use tauri::Manager;
        use tower::ServiceExt;

        struct Fixture {
            runtime: ServiceFixture,
            state: AppState,
            router: Router,
            creator: String,
            workspace: String,
            _env: codeterm_util::test_env::EnvGuard,
            _isolated_data: IsolatedDataDir,
        }

        struct IsolatedDataDir(std::path::PathBuf);

        impl Drop for IsolatedDataDir {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }

        impl Fixture {
            fn new(raw: Option<&str>) -> Self {
                let mut env = codeterm_util::test_env::EnvGuard::lock();
                crate::session::lifecycle::TabLifecycleJournal::shared();
                env.set(
                    "CODETERM_INSTANCE",
                    format!("sentinel-graph-test-{}", uuid::Uuid::new_v4().simple()),
                );
                let isolated_data = codeterm_paths::data_dir();
                assert!(isolated_data
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".codeterm-dev-sentinel-graph-test-"));
                let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
                let workspace = mux.snapshot().active_workspace_id.unwrap();
                let creator = mux.create_tab(&workspace).unwrap();
                let tab = mux.get_tab_mut(&creator).unwrap();
                tab.cwd = Some(std::env::temp_dir().to_string_lossy().into_owned());
                tab.agent_type = Some("supplied-agent".into());
                tab.session_binding_nonce = Some("fixture-nonce".into());
                let runtime = ServiceFixture::new(mux);
                let sentinel_path = runtime.temp.path().join("sentinels.json");
                if let Some(raw) = raw {
                    std::fs::write(&sentinel_path, raw).unwrap();
                }
                let handle = crate::sentinel::scheduler::fixture_handle(
                    crate::sentinel::store::Store::new(
                        sentinel_path,
                        crate::cron::clock::MockClock::starting_at(1000),
                    ),
                    runtime.service.app_handle.clone(),
                );
                runtime.service.app_handle.manage(handle);
                let mut config = crate::config::schema::AppConfig::default();
                config.api.token = Some("fixture-instance-bearer".into());
                config.mesh.enabled = true;
                let mut mesh = crate::mesh::MeshManager::new("fixture-instance".into(), 0);
                mesh.disk_persist = false;
                let state = AppState {
                    svc: Arc::new(runtime.service.clone()),
                    cron: Arc::new(parking_lot::Mutex::new(
                        crate::cron::store::CronStore::new_with_path(
                            runtime.temp.path().join("crons.json"),
                        ),
                    )),
                    cron_wake: Arc::new(std::sync::Condvar::new()),
                    api_port: 0,
                    update_state: Arc::new(parking_lot::Mutex::new(crate::update::UpdateState {
                        install_method: crate::update::InstallMethod::Unknown,
                        last_check: None,
                        last_check_time: None,
                        channel: config.updater.channel.clone(),
                        phase: crate::update::UpdatePhase::Idle,
                        last_error: None,
                        phase_changed_at: None,
                        downloaded_bytes: 0,
                        download_total_bytes: None,
                    })),
                    config: Arc::new(parking_lot::Mutex::new(config)),
                    mesh: Arc::new(parking_lot::Mutex::new(mesh)),
                    keepawake: Arc::new(parking_lot::Mutex::new(None)),
                    resize_owner: Arc::new(parking_lot::Mutex::new(Default::default())),
                    rest_api: Arc::new(crate::startup::RestApiControl::new()),
                    test_clock: None,
                    events: runtime.service.events.clone(),
                    web_dist: None,
                    snapshot_bound: Default::default(),
                };
                runtime.service.app_handle.manage(state.config.clone());
                runtime.service.app_handle.manage(state.cron.clone());
                runtime.service.app_handle.manage(state.events.clone());
                let router = crate::api::routes::create_router(state.clone());
                Self {
                    runtime,
                    state,
                    router,
                    creator,
                    workspace,
                    _env: env,
                    _isolated_data: IsolatedDataDir(isolated_data),
                }
            }

            fn create(&self) -> crate::sentinel::store::Sentinel {
                let caller = self
                    .runtime
                    .service
                    .sentinel_caller(&self.creator, Some("fixture-nonce"))
                    .unwrap();
                self.runtime.service.sentinel_create(&caller,serde_json::from_value(json!({"name":"Supplied scenario","probe":"status","question":"state?","buckets":["done"],"matches":{"done":"done"},"timeout":"1h"})).unwrap()).unwrap()
            }

            async fn request(
                &self,
                method: Method,
                path: &str,
                bearer: Option<&str>,
                body: Value,
            ) -> (StatusCode, Value) {
                let mut request = Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json");
                if let Some(bearer) = bearer {
                    request = request.header("authorization", format!("Bearer {bearer}"));
                }
                let response = self
                    .router
                    .clone()
                    .oneshot(
                        request
                            .body(Body::from(serde_json::to_vec(&body).unwrap()))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let status = response.status();
                let bytes = to_bytes(response.into_body(), 65_536).await.unwrap();
                (
                    status,
                    serde_json::from_slice(&bytes).unwrap_or(Value::Null),
                )
            }

            fn stopped(&self, id: &str) -> bool {
                self.runtime
                    .service
                    .app_handle
                    .state::<crate::sentinel::scheduler::Handle>()
                    .store
                    .lock()
                    .get(id)
                    .unwrap()
                    .machine
                    .stopped
            }
        }

        #[tokio::test]
        async fn sentinel_graph_http_router_auth_peer_refusal_and_host_stop_persist_actual_state() {
            let fixture = Fixture::new(None);
            let job = fixture.create();
            let get = format!(
                "/api/automation/graph?workspace_id={}&team_id=",
                fixture.workspace
            );
            let stop = format!("/api/automation/blueprints/{}/stop", job.id);
            let body = json!({"workspace_id":fixture.workspace,"team_id":null});
            for token in [None, Some("invalid-token")] {
                assert_eq!(
                    fixture
                        .request(Method::GET, &get, token, Value::Null)
                        .await
                        .0,
                    StatusCode::UNAUTHORIZED
                );
                assert_eq!(
                    fixture
                        .request(Method::POST, &stop, token, body.clone())
                        .await
                        .0,
                    StatusCode::UNAUTHORIZED
                );
            }
            let (status, graph) = fixture
                .request(
                    Method::GET,
                    &get,
                    Some("fixture-instance-bearer"),
                    Value::Null,
                )
                .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(graph["scenarios"][0]["root"]["id"], job.id);
            assert!(graph["nodes"][0]["stop"]["allowed"].as_bool().unwrap());
            assert!(!serde_json::to_string(&graph)
                .unwrap()
                .contains("fixture-nonce"));
            let (private_key, public_key) = crate::mesh::crown::generate_peer_keypair();
            fixture.state.mesh.lock().paired_peers.insert(
                "fixture-peer".into(),
                crate::mesh::types::PairedPeer {
                    instance_id: "fixture-peer".into(),
                    instance_name: "Peer".into(),
                    addresses: vec![],
                    api_port: 0,
                    public_key,
                    paired_at: "now".into(),
                    last_seen: None,
                    online: true,
                    is_primary: false,
                    access_level: crate::mesh::types::AccessLevel::Full,
                    version: None,
                    web_ui: None,
                    relay_protocol_version: crate::mesh::types::RELAY_PROTOCOL_VERSION,
                    consecutive_misses: 0,
                    last_active_ms: None,
                },
            );
            let token = crate::mesh::crown::sign_request(&private_key, "fixture-peer").unwrap();
            assert_eq!(
                fixture
                    .request(Method::POST, &stop, Some(&token), body.clone())
                    .await
                    .0,
                StatusCode::FORBIDDEN
            );
            assert!(!fixture.stopped(&job.id));
            assert_eq!(
                fixture
                    .request(Method::GET, &get, Some(&token), Value::Null)
                    .await
                    .1["nodes"][0]["stop"]["allowed"],
                false
            );
            let handle = fixture
                .runtime
                .service
                .app_handle
                .state::<crate::sentinel::scheduler::Handle>();
            let lease = handle.store.lock().due().unwrap().remove(0);
            let (status, result) = fixture
                .request(Method::POST, &stop, Some("fixture-instance-bearer"), body)
                .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(result["reason"], "stopped_by_host");
            assert!(lease.is_cancelled());
            let restored = crate::sentinel::store::Store::new(
                fixture.runtime.temp.path().join("sentinels.json"),
                crate::cron::clock::MockClock::starting_at(1000),
            );
            assert_eq!(
                restored.get(&job.id).unwrap().last_reason.as_deref(),
                Some("stopped_by_host")
            );
            assert_eq!(
                fixture
                    .request(
                        Method::GET,
                        &get,
                        Some("fixture-instance-bearer"),
                        Value::Null
                    )
                    .await
                    .1["nodes"][0]["stop"]["allowed"],
                false
            );
        }

        #[tokio::test]
        async fn sentinel_graph_delete_removes_the_graph_from_the_list_and_persisted_file() {
            let fixture = Fixture::new(None);
            let job = fixture.create();
            let path = format!("/api/automation/blueprints/{}", job.id);
            let body = json!({"workspace_id":fixture.workspace,"team_id":null});
            assert_eq!(
                fixture
                    .request(Method::DELETE, &path, Some("fixture-instance-bearer"), body,)
                    .await
                    .0,
                StatusCode::NO_CONTENT
            );
            let (status, graph) = fixture
                .request(
                    Method::GET,
                    &format!(
                        "/api/automation/graph?workspace_id={}&team_id=",
                        fixture.workspace
                    ),
                    Some("fixture-instance-bearer"),
                    Value::Null,
                )
                .await;
            assert_eq!(status, StatusCode::OK);
            assert!(graph["scenarios"].as_array().unwrap().is_empty());
            let persisted: Value = serde_json::from_slice(
                &std::fs::read(fixture.runtime.temp.path().join("sentinels.json")).unwrap(),
            )
            .unwrap();
            assert!(persisted.as_array().unwrap().is_empty());
        }

        #[tokio::test]
        async fn legacy_sentinel_rest_alias_deletes_the_persisted_blueprint() {
            let fixture = Fixture::new(None);
            let job = fixture.create();
            let path = format!("/api/automation/sentinels/{}", job.id);
            let body = json!({"workspace_id":fixture.workspace,"team_id":null});
            assert_eq!(
                fixture
                    .request(Method::DELETE, &path, Some("fixture-instance-bearer"), body,)
                    .await
                    .0,
                StatusCode::NO_CONTENT
            );
            let (status, graph) = fixture
                .request(
                    Method::GET,
                    &format!(
                        "/api/automation/graph?workspace_id={}&team_id=",
                        fixture.workspace
                    ),
                    Some("fixture-instance-bearer"),
                    Value::Null,
                )
                .await;
            assert_eq!(status, StatusCode::OK);
            assert!(graph["scenarios"].as_array().unwrap().is_empty());
            let persisted: Value = serde_json::from_slice(
                &std::fs::read(fixture.runtime.temp.path().join("sentinels.json")).unwrap(),
            )
            .unwrap();
            assert!(persisted.as_array().unwrap().is_empty());
        }

        #[tokio::test]
        async fn sentinel_graph_http_declared_tab_never_falls_back_after_failed_nonce_or_creator_check(
        ) {
            let fixture = Fixture::new(None);
            let job = fixture.create();
            let path = format!("/api/automation/blueprints/{}/stop", job.id);
            for nonce in [Value::Null, json!("stale-nonce")] {
                let body = json!({"workspace_id":fixture.workspace,"team_id":null,"caller_tab_id":fixture.creator,"session_binding_nonce":nonce});
                assert_eq!(
                    fixture
                        .request(Method::POST, &path, Some("fixture-instance-bearer"), body)
                        .await
                        .0,
                    StatusCode::BAD_REQUEST
                );
                assert!(!fixture.stopped(&job.id));
            }
            let other = fixture
                .runtime
                .service
                .mux
                .write()
                .create_tab(&fixture.workspace)
                .unwrap();
            {
                let mut mux = fixture.runtime.service.mux.write();
                let tab = mux.get_tab_mut(&other).unwrap();
                tab.cwd = Some(std::env::temp_dir().to_string_lossy().into_owned());
                tab.session_binding_nonce = Some("foreign-current-nonce".into());
            }
            let foreign = json!({"workspace_id":fixture.workspace,"team_id":null,"caller_tab_id":other,"session_binding_nonce":"foreign-current-nonce"});
            assert_eq!(
                fixture
                    .request(
                        Method::POST,
                        &path,
                        Some("fixture-instance-bearer"),
                        foreign
                    )
                    .await
                    .0,
                StatusCode::BAD_REQUEST
            );
            assert!(!fixture.stopped(&job.id));
            let body = json!({"workspace_id":fixture.workspace,"team_id":null,"caller_tab_id":fixture.creator,"session_binding_nonce":"fixture-nonce"});
            let (status, result) = fixture
                .request(Method::POST, &path, Some("fixture-instance-bearer"), body)
                .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(result["reason"], "stopped_by_creator");
        }

        #[tokio::test]
        async fn sentinel_graph_http_unreadable_inventory_is_unavailable_and_never_overwritten() {
            for raw in [
                "{invalid-json",
                r#"[{"id":"old","routes":{"done":{"kind":"notify"}}}]"#,
            ] {
                let fixture = Fixture::new(Some(raw));
                let get = format!(
                    "/api/automation/graph?workspace_id={}&team_id=",
                    fixture.workspace
                );
                let (status, error) = fixture
                    .request(
                        Method::GET,
                        &get,
                        Some("fixture-instance-bearer"),
                        Value::Null,
                    )
                    .await;
                assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
                assert!(error["error"]["code"]
                    .as_str()
                    .unwrap()
                    .starts_with("blueprint_store_"));
                let body = json!({"workspace_id":fixture.workspace,"team_id":null});
                assert_eq!(
                    fixture
                        .request(
                            Method::POST,
                            "/api/automation/blueprints/old/stop",
                            Some("fixture-instance-bearer"),
                            body
                        )
                        .await
                        .0,
                    StatusCode::SERVICE_UNAVAILABLE
                );
                assert_eq!(
                    std::fs::read(fixture.runtime.temp.path().join("sentinels.json")).unwrap(),
                    raw.as_bytes()
                );
            }
        }
        #[tokio::test]
        async fn sentinel_graph_actual_connected_cron_change_emits_scoped_invalidation_then_current_snapshot(
        ) {
            use tauri::Listener;
            let fixture = Fixture::new(None);
            let cron=fixture.state.cron.lock().create(serde_json::from_value(json!({"name":"nightly-ci-review","workspace_id":fixture.workspace,"creator_tab_id":fixture.creator,"schedule":"0 * * * *","task":"private task"})).unwrap()).unwrap();
            let caller = fixture
                .runtime
                .service
                .sentinel_caller(&fixture.creator, Some("fixture-nonce"))
                .unwrap();
            fixture.runtime.service.sentinel_create(&caller,serde_json::from_value(json!({"name":"cron-result-watch","probe":"status","question":"state?","buckets":["done"],"matches":{"done":"done"},"timeout":"1h","routes":{"done":format!("cron:{}",cron.id)}})).unwrap()).unwrap();
            let before = fixture
                .runtime
                .service
                .automation_graph_snapshot(&fixture.workspace, None)
                .unwrap();
            assert_eq!(
                before
                    .nodes
                    .iter()
                    .find(|node| node["identity"]["id"] == cron.id)
                    .unwrap()["state"],
                "enabled"
            );
            let handle = fixture
                .runtime
                .service
                .app_handle
                .state::<crate::sentinel::scheduler::Handle>();
            crate::service::sentinel::graph::install_invalidations(
                &fixture.runtime.service,
                handle.store.clone(),
            );
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let listener = fixture.runtime.service.app_handle.listen_any(
                "automation-graph-changed",
                move |event| {
                    tx.send(serde_json::from_str::<Value>(event.payload()).unwrap())
                        .unwrap();
                },
            );
            fixture
                .runtime
                .service
                .cron_enable(
                    &fixture.state.cron,
                    &fixture.state.cron_wake,
                    &cron.id,
                    false,
                )
                .unwrap();
            let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(event["workspace_id"], fixture.workspace);
            assert_eq!(event["team_id"], Value::Null);
            assert_eq!(event["revision"], before.revision);
            let after = fixture
                .runtime
                .service
                .automation_graph_snapshot(&fixture.workspace, None)
                .unwrap();
            assert_eq!(
                after
                    .nodes
                    .iter()
                    .find(|node| node["identity"]["id"] == cron.id)
                    .unwrap()["state"],
                "disabled"
            );
            fixture.runtime.service.app_handle.unlisten(listener);
        }
        #[tokio::test]
        async fn sentinel_graph_actual_connected_worker_status_event_refreshes_authoritative_snapshot(
        ) {
            use crate::agent::status::TabStatus;
            use tauri::Listener;
            let fixture = Fixture::new(None);
            let worker = fixture
                .runtime
                .service
                .mux
                .write()
                .create_tab(&fixture.workspace)
                .unwrap();
            {
                let mut mux = fixture.runtime.service.mux.write();
                let tab = mux.get_tab_mut(&worker).unwrap();
                tab.agent_type = Some("supplied-agent".into());
                mux.link_pty(&worker, "fixture-graph-pty".into());
                assert_eq!(
                    mux.tab_id_for_pty("fixture-graph-pty"),
                    Some(worker.clone())
                );
            }
            fixture
                .runtime
                .service
                .agent_tracker
                .lock()
                .set_idle("fixture-graph-pty");
            let caller = fixture
                .runtime
                .service
                .sentinel_caller(&fixture.creator, Some("fixture-nonce"))
                .unwrap();
            fixture.runtime.service.sentinel_create(&caller,serde_json::from_value(json!({"name":"agent-result-notify","probe":"status","question":"state?","buckets":["done"],"matches":{"done":"done"},"timeout":"1h","routes":{"done":format!("send:{worker}")}})).unwrap()).unwrap();
            let before = fixture
                .runtime
                .service
                .automation_graph_snapshot(&fixture.workspace, None)
                .unwrap();
            assert_eq!(
                before
                    .nodes
                    .iter()
                    .find(|node| node["identity"]["id"] == worker)
                    .unwrap()["state"],
                "idle"
            );
            let handle = fixture
                .runtime
                .service
                .app_handle
                .state::<crate::sentinel::scheduler::Handle>();
            crate::service::sentinel::graph::install_invalidations(
                &fixture.runtime.service,
                handle.store.clone(),
            );
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let listener = fixture.runtime.service.app_handle.listen_any(
                "automation-graph-changed",
                move |event| {
                    tx.send(serde_json::from_str::<Value>(event.payload()).unwrap())
                        .unwrap();
                },
            );
            fixture
                .runtime
                .service
                .agent_tracker
                .lock()
                .set_working("fixture-graph-pty");
            fixture.runtime.service.emit_tab_status_if_changed(
                "fixture-graph-pty",
                TabStatus::Idle,
                TabStatus::Working,
            );
            let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(event["workspace_id"], fixture.workspace);
            assert_eq!(event["team_id"], Value::Null);
            let after = fixture
                .runtime
                .service
                .automation_graph_snapshot(&fixture.workspace, None)
                .unwrap();
            assert_eq!(
                after
                    .nodes
                    .iter()
                    .find(|node| node["identity"]["id"] == worker)
                    .unwrap()["state"],
                "working"
            );
            fixture.runtime.service.app_handle.unlisten(listener);
        }
    }
}
