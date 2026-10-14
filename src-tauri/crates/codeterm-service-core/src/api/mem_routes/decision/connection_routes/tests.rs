use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request,
    routing::{delete, get, post, put},
    Router,
};
use codeterm_decision::{
    DecisionAnswer, DecisionModel, DecisionError, DecisionModelInfo, DecisionPrimitives, DecisionRequest,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tower::ServiceExt;

struct Model {
    id: String,
    mode: TokenMode,
    selected: parking_lot::Mutex<String>,
    unavailable: AtomicBool,
}
impl DecisionModel for Model {
    fn id(&self) -> &str {
        &self.id
    }
    fn token_mode(&self) -> TokenMode {
        self.mode
    }
    fn model_id(&self) -> Option<String> {
        Some(self.selected.lock().clone())
    }
    fn primitives(&self) -> DecisionPrimitives {
        Default::default()
    }
    fn models(&self) -> Vec<DecisionModelInfo> {
        ["one", "two"]
            .into_iter()
            .map(|id| DecisionModelInfo {
                id: id.into(),
                display_name: id.into(),
            })
            .collect()
    }
    fn select_model(&self, id: &str) -> bool {
        if self.unavailable.load(Ordering::SeqCst) {
            return false;
        }
        *self.selected.lock() = id.into();
        true
    }
    fn decide(
        &self,
        _: &DecisionRequest,
        _: Instant,
    ) -> std::result::Result<DecisionAnswer, DecisionError> {
        Err(DecisionError::Unsupported)
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    state: ConnectionState,
    router: Router,
    models: Vec<Arc<Model>>,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = Arc::new(parking_lot::Mutex::new(
            crate::config::schema::AppConfig::default(),
        ));
        let registry = Arc::new(SupporterRegistry::new());
        let models: Vec<_> = [
            ("required", TokenMode::Required),
            ("optional", TokenMode::Optional),
            ("none", TokenMode::None),
        ]
        .into_iter()
        .map(|(id, mode)| {
            Arc::new(Model {
                id: id.into(),
                mode,
                selected: parking_lot::Mutex::new("one".into()),
                unavailable: AtomicBool::new(false),
            })
        })
        .collect();
        for model in &models {
            registry.register(model.id(), model.clone());
        }
        let path = dir.path().join("config.toml");
        let saved = config.clone();
        let persist: Persist = Arc::new(move |cfg| {
            std::fs::write(&path, toml::to_string(cfg).unwrap())
                .map_err(|_| ConnectionError::unavailable())?;
            saved.lock().supporter = cfg.clone();
            Ok(())
        });
        let state = ConnectionState {
            config,
            registry,
            store: TokenStore::new(dir.path().join("supporter")),
            runtime: Arc::new(crate::service::supporter::SupporterRuntime::new(
                dir.path().join("supporter"),
            )),
            persist,
        };
        let router = Router::new()
            .route(
                "/api/supporter/connections",
                get(connections_list).post(connections_connect),
            )
            .route(
                "/api/supporter/connections/{adapter}",
                delete(connections_disconnect),
            )
            .route(
                "/api/supporter/connections/{adapter}/activate",
                post(connections_activate),
            )
            .route(
                "/api/supporter/connections/{adapter}/model",
                put(connections_model),
            )
            .route(
                "/api/supporter/connections/{adapter}/token",
                post(connections_token),
            )
            .with_state(state.clone());
        Self {
            _dir: dir,
            state,
            router,
            models,
        }
    }
    async fn request(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(format!("/api/supporter/connections{path}"))
            .header("Content-Type", "application/json")
            .body(if body.is_null() {
                Body::empty()
            } else {
                Body::from(body.to_string())
            })
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        assert_eq!(response.headers()["content-type"], "application/json");
        let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }
    async fn connect(&self, adapter: &str, token: Option<&str>) -> (StatusCode, Value) {
        self.request(
            "POST",
            "",
            json!({ "adapter": adapter, "model": "one", "token": token, "activate": true }),
        )
        .await
    }
}

#[tokio::test]
async fn supporter_connections_routes_connect_list_activate_model_replace_disconnect() {
    let f = Fixture::new();
    assert_eq!(
        f.connect("required", None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.connect("required", Some("private-secret")).await.0,
        StatusCode::OK
    );
    assert_eq!(f.connect("optional", None).await.0, StatusCode::OK);
    assert_eq!(
        f.request("PUT", "/required/model", json!({ "model": "two" }))
            .await
            .0,
        StatusCode::OK
    );
    let (_, rows) = f.request("GET", "", Value::Null).await;
    assert_eq!(rows["active"], "optional");
    assert_eq!(rows["connections"][0]["adapter"], "optional");
    assert!(!rows.to_string().contains("private-secret"));
    assert_eq!(
        f.request("POST", "/required/activate", Value::Null).await.0,
        StatusCode::OK
    );
    assert_eq!(f.models[0].model_id().as_deref(), Some("two"));
    assert_eq!(
        f.request(
            "POST",
            "/required/token",
            json!({ "token": "replacement-secret" })
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.state
            .config
            .lock()
            .supporter
            .connection("required")
            .model
            .as_deref(),
        Some("two")
    );
    assert_eq!(
        f.state.config.lock().supporter.active_id(),
        Some("required")
    );
    assert_eq!(
        f.request("DELETE", "/optional", Value::Null).await.0,
        StatusCode::OK
    );
    assert_eq!(
        f.state.config.lock().supporter.active_id(),
        Some("required")
    );
    assert_eq!(
        f.request("DELETE", "/required", Value::Null).await.0,
        StatusCode::OK
    );
    assert_eq!(f.state.config.lock().supporter.adapter, None);
    assert!(f.state.store.load("required").unwrap().is_none());
    assert_eq!(
        f.request("GET", "", Value::Null).await.1["connections"],
        json!([])
    );
}

#[tokio::test]
async fn supporter_connections_routes_partial_failure_retry_preserves_old_active() {
    let f = Fixture::new();
    f.connect("none", None).await;
    f.models[0].unavailable.store(true, Ordering::SeqCst);
    let (status, saved) = f.connect("required", Some("private-secret")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["state"], "saved_not_active");
    assert_eq!(f.state.config.lock().supporter.active_id(), Some("none"));
    assert_eq!(
        f.request("POST", "/required/activate", Value::Null).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(f.state.config.lock().supporter.active_id(), Some("none"));
    f.models[0].unavailable.store(false, Ordering::SeqCst);
    assert_eq!(
        f.request("POST", "/required/activate", Value::Null).await.0,
        StatusCode::OK
    );
    assert_eq!(
        f.state.config.lock().supporter.active_id(),
        Some("required")
    );
}

#[tokio::test]
async fn supporter_connections_routes_none_token_policy_and_errors() {
    let f = Fixture::new();
    let (status, error) = f.connect("none", Some("private-secret")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error["error"]["code"], "authentication_failed");
    assert_eq!(
        f.request(
            "POST",
            "",
            json!({ "adapter": "missing", "activate": true })
        )
        .await
        .1["error"]["code"],
        "unknown_adapter"
    );
    assert_eq!(
        f.request(
            "POST",
            "",
            json!({ "adapter": "none", "model": "missing", "activate": true })
        )
        .await
        .1["error"]["code"],
        "invalid_model"
    );
    assert_eq!(f.connect("none", None).await.0, StatusCode::OK);
    assert_eq!(
        f.request("DELETE", "/none", Value::Null).await.0,
        StatusCode::OK
    );
    assert_eq!(f.state.config.lock().supporter.adapter, None);
}
