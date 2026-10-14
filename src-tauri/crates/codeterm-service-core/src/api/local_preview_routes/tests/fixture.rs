use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{body::Body, http::Request, response::Response, Router};
use parking_lot::Mutex;
use tower::ServiceExt;

use crate::agent::chat_types::{ChatMsg, StructuredChat};
use crate::agent::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};
use crate::api::routes::AppState;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

struct TranscriptProvider(Arc<Mutex<HashMap<String, Vec<ChatMsg>>>>);

impl AgentProvider for TranscriptProvider {
    fn id(&self) -> &str {
        "preview-fixture"
    }
    fn display_name(&self) -> &str {
        "Preview Fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn detect_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
    ) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn uses_structured_chat(&self) -> bool {
        true
    }
    fn read_structured_chat(&self, _: &str, session: &str, _: Option<&str>) -> StructuredChat {
        StructuredChat {
            messages: self.0.lock().get(session).cloned().unwrap_or_default(),
            ..Default::default()
        }
    }
}

fn app_state(fixture: &ServiceFixture) -> AppState {
    let mut config = crate::config::schema::AppConfig::default();
    config.api.token = Some("preview-test-token".into());
    config.api.preview_secret_paths = vec![fixture
        .temp
        .path()
        .join("private")
        .to_string_lossy()
        .into_owned()];
    let config = Arc::new(parking_lot::Mutex::new(config));
    let mut mesh = crate::mesh::MeshManager::new("local-preview-test".into(), 0);
    mesh.disk_persist = false;
    AppState {
        svc: Arc::new(fixture.service.clone()),
        cron: Arc::new(parking_lot::Mutex::new(
            crate::cron::store::CronStore::new_with_path(fixture.temp.path().join("crons.json")),
        )),
        cron_wake: Arc::new(std::sync::Condvar::new()),
        api_port: 0,
        update_state: Arc::new(parking_lot::Mutex::new(crate::update::UpdateState {
            install_method: crate::update::InstallMethod::Unknown,
            last_check: None,
            last_check_time: None,
            channel: crate::update::Channel::default(),
            phase: crate::update::UpdatePhase::Idle,
            last_error: None,
            phase_changed_at: None,
            downloaded_bytes: 0,
            download_total_bytes: None,
        })),
        config,
        mesh: Arc::new(parking_lot::Mutex::new(mesh)),
        keepawake: Arc::new(parking_lot::Mutex::new(None)),
        resize_owner: Arc::new(parking_lot::Mutex::new(Default::default())),
        rest_api: Arc::new(crate::startup::RestApiControl::new()),
        test_clock: None,
        events: fixture.service.events.clone(),
        web_dist: None,
        snapshot_bound: crate::startup::snapshot_bound::SnapshotBound::default(),
    }
}

pub(super) struct Fixture {
    _service: ServiceFixture,
    pub app: Router,
    pub owner: String,
    pub sibling: String,
    pub root: PathBuf,
    rows: Arc<Mutex<HashMap<String, Vec<ChatMsg>>>>,
    state: AppState,
}

impl Fixture {
    pub fn new() -> Self {
        let now = std::time::Instant::now();
        Self::with_clock(Arc::new(move || now))
    }

    pub fn with_clock(clock: Arc<dyn Fn() -> std::time::Instant + Send + Sync>) -> Self {
        let (mut mux, owner) = MultiplexerState::new();
        let sibling = mux.create_tab_record();
        for id in [&owner, &sibling] {
            let tab = mux.get_tab_mut(id).unwrap();
            tab.agent_type = Some("preview-fixture".into());
            tab.agent_session_id = Some(id.clone());
        }
        let service = ServiceFixture::new(mux);
        let rows = Arc::new(Mutex::new(HashMap::new()));
        service
            .service
            .agent_registry
            .register_provider(Box::new(TranscriptProvider(rows.clone())));
        let root = crate::service::local_preview::html_preview::preview_test_root(service.temp.path());
        let state = app_state(&service);
        let app = crate::api::routes::create_router_with_preview_admission(
            state.clone(),
            None,
            Arc::new(super::Admission::with_clock(clock.clone())),
            Arc::new(super::Admission::with_clock(clock)),
        );
        Self {
            _service: service,
            app,
            owner,
            sibling,
            root,
            rows,
            state,
        }
    }

    pub fn isolated_peer(&self) -> String {
        use crate::mesh::types::{AccessLevel, PairedPeer, RELAY_PROTOCOL_VERSION};
        let (private, public) = crate::mesh::crown::generate_peer_keypair();
        self.state.config.lock().mesh.enabled = true;
        self._service
            .service
            .mux
            .write()
            .get_tab_mut(&self.owner)
            .unwrap()
            .ownership = Some(codeterm_types::pty::RemoteOwnership {
            controller_title: "Preview".into(),
            controller_tab_id: None,
            peer_id: "preview-peer".into(),
            peer_name: "Preview".into(),
            team: None,
        });
        self.state.mesh.lock().paired_peers.insert(
            "preview-peer".into(),
            PairedPeer {
                instance_id: "preview-peer".into(),
                instance_name: "Preview".into(),
                addresses: Vec::new(),
                api_port: 0,
                public_key: public,
                paired_at: "now".into(),
                last_seen: None,
                online: true,
                is_primary: false,
                access_level: AccessLevel::AgentIsolated,
                version: None,
                web_ui: None,
                relay_protocol_version: RELAY_PROTOCOL_VERSION,
                consecutive_misses: 0,
                last_active_ms: None,
            },
        );
        private
    }

    pub fn reference(&self, tab: &str, path: &Path) {
        self.rows
            .lock()
            .entry(tab.into())
            .or_default()
            .push(ChatMsg {
                kind: "assistant".into(),
                content: format!("@ds-document {}", path.display()),
                ..Default::default()
            });
    }

    pub fn write(&self, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }

    pub async fn post(
        &self,
        tab: &str,
        suffix: &str,
        body: serde_json::Value,
        auth: Option<&str>,
    ) -> Response {
        let mut request = Request::builder()
            .method("POST")
            .uri(format!("/api/tabs/{tab}/preview/{suffix}"))
            .header("content-type", "application/json");
        if let Some(auth) = auth {
            request = request.header("authorization", auth);
        }
        self.app
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }

    pub async fn read(&self, tab: &str, path: &Path, kind: &str) -> Response {
        self.post(
            tab,
            "read",
            serde_json::json!({"path": path, "kind": kind}),
            Some("Bearer preview-test-token"),
        )
        .await
    }
}

pub(super) fn directory_link(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("cmd");
        crate::process_util::no_window_std(&mut command);
        let result = command
            .args(["/D", "/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "junction fixture failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
