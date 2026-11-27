//! Two-peer registered-router integration for chat preview blocks over mesh relay.

use std::io::{BufRead, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use base64::Engine;
use codeterm_api::routes::{self, AppState};
use codeterm_service_core::{
    agent::{
        chat_types::{MessageBlock, RichMessage, SessionDelta},
        provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery},
    },
    mesh::{
        crown,
        types::{AccessLevel, PairedPeer, PeerAddress, RELAY_PROTOCOL_VERSION},
    },
    multiplexer::state::MultiplexerState,
    service::test_fixture::ServiceFixture,
};
use serde_json::{json, Value};
use tower::ServiceExt;

#[path = "../src/test_support.rs"]
mod test_support;

struct JsonlPreviewProvider;

impl AgentProvider for JsonlPreviewProvider {
    fn id(&self) -> &str {
        "mesh-preview-jsonl"
    }
    fn display_name(&self) -> &str {
        "Mesh Preview JSONL"
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
    fn session_file_path(&self, cwd: &str, session: &str) -> Option<PathBuf> {
        Some(Path::new(cwd).join(format!("{session}.jsonl")))
    }
    fn session_jsonl_path(&self, cwd: &str, session: &str) -> Option<PathBuf> {
        self.session_file_path(cwd, session)
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
    fn parse_session_delta_bounded(
        &self,
        path: &Path,
        from: u64,
        budget: u64,
    ) -> Option<SessionDelta> {
        let mut file = std::fs::File::open(path).ok()?;
        file.seek(SeekFrom::Start(from)).ok()?;
        let mut reader = std::io::BufReader::new(file);
        let mut offset = from;
        let mut messages = Vec::new();
        while offset - from < budget {
            let mut line = String::new();
            if reader.read_line(&mut line).ok()? == 0 || !line.ends_with('\n') {
                break;
            }
            let content: String = serde_json::from_str(&line).ok()?;
            messages.push(RichMessage {
                role: "assistant".into(),
                record_start: Some(offset),
                blocks: vec![MessageBlock {
                    kind: "text".into(),
                    data: json!({"text": content}),
                }],
                ..Default::default()
            });
            offset += line.len() as u64;
        }
        Some(SessionDelta {
            messages,
            new_offset: offset,
        })
    }
}

fn initialize() {
    static ROOT: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config");
        let data = root.path().join("data");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::env::set_var("CODETERM_CONFIG_DIR", config);
        std::env::set_var("CODETERM_DATA_DIR", data);
        root
    });
    test_support::install_app_version();
}

fn state(fixture: &ServiceFixture, id: &str, port: u16) -> AppState {
    let mut state = test_support::app_state(fixture);
    state.api_port = port;
    state.config.lock().api.token = Some("owner-secret".into());
    state.config.lock().mesh.enabled = true;
    let (key, public) = crown::generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        mesh.paired_peers.clear();
        mesh.ident.identity.instance_id = id.into();
        mesh.ident.identity.private_key = Some(key);
        mesh.ident.identity.public_key = Some(public);
    }
    state
}

fn pair(target: &AppState, source: &AppState) {
    let mesh = source.mesh.lock();
    target.mesh.lock().insert_account_peer_for_test(PairedPeer {
        instance_id: mesh.ident.identity.instance_id.clone(),
        instance_name: mesh.ident.identity.instance_id.clone(),
        addresses: vec![PeerAddress::from_ip("127.0.0.1")],
        api_port: source.api_port,
        public_key: mesh.ident.identity.public_key.clone().unwrap(),
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::Full,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    });
}

async fn request(
    app: axum::Router,
    token: &str,
    method: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("x-codeterm-authenticated-origin", "Owner")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn relay(app: axum::Router, token: &str, method: &str, path: &str, body: Value) -> Value {
    let (status, response) = request(
        app,
        token,
        "POST",
        "/api/mesh/relay-to",
        json!({
            "target_peer_id": "target",
            "method": method,
            "path": path,
            "body": body,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{path}: {response}");
    assert_eq!(response["status"], 200, "{path}: {response}");
    assert_eq!(response["ok"], true, "{path}: {response}");
    response["data"].clone()
}

#[tokio::test]
async fn mesh_owner_relay_opens_jsonl_chat_page_document_image_and_canvas_while_refusing_agents_and_unreferenced() {
    initialize();
    let (source_mux, source_tab) = MultiplexerState::new();
    let source_fixture = ServiceFixture::new(source_mux);
    let (mut target_mux, target_tab) = MultiplexerState::new();
    target_mux.get_tab_mut(&target_tab).unwrap().agent_type = Some("mesh-preview-jsonl".into());
    target_mux.get_tab_mut(&target_tab).unwrap().agent_session_id = Some("mesh-session".into());
    target_mux
        .get_tab_mut(&target_tab)
        .unwrap()
        .session_binding_nonce = Some("mesh-nonce".into());
    let target_fixture = ServiceFixture::new(target_mux);
    let cwd = target_fixture.temp.path().to_string_lossy().into_owned();
    target_fixture
        .service
        .mux
        .write()
        .get_tab_mut(&target_tab)
        .unwrap()
        .cwd = Some(cwd);
    target_fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsonlPreviewProvider));

    let root = codeterm_service_core::service::local_preview::html_preview::preview_test_root(
        target_fixture.temp.path(),
    );
    let page = root.join("mesh-page.html");
    let doc = root.join("mesh-doc.md");
    let img = root.join("mesh-img.png");
    let canvas_img = root.join("canvas-img.png");
    let canvas = root.join("board.canvas.json");
    let unreferenced = root.join("secret-other.html");
    let png_bytes = base64::prelude::BASE64_STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGNgYPgPAAEDAQAIicLsAAAAAElFTkSuQmCC")
        .unwrap();
    std::fs::write(&page, "<main>mesh html page</main>").unwrap();
    std::fs::write(&doc, "# mesh doc").unwrap();
    std::fs::write(&img, &png_bytes).unwrap();
    std::fs::write(&canvas_img, &png_bytes).unwrap();
    std::fs::write(
        &canvas,
        json!({
            "format": "domios.canvas/1",
            "title": "Mesh board",
            "items": [{"id": "pic", "kind": "image", "src": canvas_img, "x": 0, "y": 0, "w": 100, "h": 100}]
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(&unreferenced, "<main>unreferenced</main>").unwrap();

    let transcript = target_fixture.temp.path().join("mesh-session.jsonl");
    let message = format!(
        "<ds-page src=\"{}\" />\n\n<ds-document src=\"{}\" />\n\n<ds-image src=\"{}\" />\n\n<ds-canvas src=\"{}\" />",
        page.display(),
        doc.display(),
        img.display(),
        canvas.display(),
    );
    let mut file = std::fs::File::create(&transcript).unwrap();
    writeln!(file, "{}", serde_json::to_string(&message).unwrap()).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source = state(&source_fixture, "source", 0);
    let target = state(
        &target_fixture,
        "target",
        listener.local_addr().unwrap().port(),
    );
    pair(&source, &target);
    pair(&target, &source);
    let source_agent = source
        .svc
        .frontend_pty_env(Some(&source_tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let target_app = routes::create_router_with_frontend(target.clone(), None);
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            target_app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .await
        .unwrap();
    });

    let app = routes::create_router_with_frontend(source.clone(), None);
    // Owner relay: prepare settles directly to "ready" without needing an SSE channel.
    let prepared = relay(
        app.clone(),
        "owner-secret",
        "POST",
        &format!("/api/tabs/{target_tab}/preview/prepare"),
        json!({}),
    )
    .await;
    assert_eq!(prepared["state"], "ready", "{prepared}");

    let read_path = format!("/api/tabs/{target_tab}/preview/read");
    let got_page = relay(
        app.clone(),
        "owner-secret",
        "POST",
        &read_path,
        json!({"path": page, "kind": "page"}),
    )
    .await;
    assert_eq!(got_page["data"], "<main>mesh html page</main>");
    let got_doc = relay(
        app.clone(),
        "owner-secret",
        "POST",
        &read_path,
        json!({"path": doc, "kind": "document"}),
    )
    .await;
    assert_eq!(got_doc["data"], "# mesh doc");
    let got_img = relay(
        app.clone(),
        "owner-secret",
        "POST",
        &read_path,
        json!({"path": img, "kind": "image"}),
    )
    .await;
    assert!(got_img["data"]
        .as_str()
        .unwrap()
        .starts_with("data:image/jpeg;base64,"));
    let got_canvas = relay(
        app.clone(),
        "owner-secret",
        "POST",
        &read_path,
        json!({"path": canvas, "kind": "canvas"}),
    )
    .await;
    assert!(got_canvas["data"].as_str().unwrap().contains("Mesh board"));
    let got_canvas_img = relay(
        app.clone(),
        "owner-secret",
        "POST",
        &read_path,
        json!({"path": canvas_img, "kind": "image", "canvas": canvas}),
    )
    .await;
    assert!(got_canvas_img["data"]
        .as_str()
        .unwrap()
        .starts_with("data:image/jpeg;base64,"));

    // Unreferenced file stays 404 even for the owner.
    let (status, unref_resp) = request(
        app.clone(),
        "owner-secret",
        "POST",
        "/api/mesh/relay-to",
        json!({
            "target_peer_id": "target",
            "method": "POST",
            "path": read_path,
            "body": {"path": unreferenced, "kind": "page"},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unref_resp["status"], 404, "{unref_resp}");

    // Relayed remote agent is refused with 403 on prepare and read.
    for (suffix, body) in [
        ("prepare", json!({})),
        ("read", json!({"path": page, "kind": "page"})),
    ] {
        let (status, agent_resp) = request(
            app.clone(),
            &source_agent,
            "POST",
            "/api/mesh/relay-to",
            json!({
                "target_peer_id": "target",
                "method": "POST",
                "path": format!("/api/tabs/{target_tab}/preview/{suffix}"),
                "body": body,
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{suffix}: {agent_resp}");
        assert_eq!(agent_resp["status"], 403, "{suffix}: {agent_resp}");
    }

    stop.send(()).unwrap();
    server.await.unwrap();
}
