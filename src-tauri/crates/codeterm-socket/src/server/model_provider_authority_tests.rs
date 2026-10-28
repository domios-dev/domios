use super::*;
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::Request,
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState,
    service::test_fixture::{model_provider, ServiceFixture},
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

const OWNER: &str = "model-provider-owner";
const HARNESS: &str = "authority-fixture";

fn fixture() -> (
    ServiceFixture,
    ServiceState,
    String,
    Arc<model_provider::Calls>,
) {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let calls = model_provider::install(&fixture);
    let svc = Arc::new(fixture.service.clone());
    svc.ext::<codeterm_service_core::config::ConfigState>()
        .lock()
        .api
        .token = Some(OWNER.into());
    let token = svc
        .frontend_pty_env(Some(&tab))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    (fixture, svc, token, calls)
}

fn discovery() -> Value {
    json!({
        "harness": HARNESS, "kind": "openai-compatible",
        "base_url": "https://caller-chosen.invalid/v1",
        "source_secret": "named-vault-secret"
    })
}

fn denied_requests() -> Vec<(&'static str, Value)> {
    let provider = json!({
        "id": "endpoint", "kind": "openai-compatible",
        "baseUrl": "https://caller-chosen.invalid/v1",
        "models": ["fixture-model"], "secretName": "slot"
    });
    vec![
        ("model_provider.discover", discovery()),
        (
            "model_provider.discover",
            json!({
                "harness": HARNESS, "source_secret": "named-vault-secret",
                "caller_class": "Local", "authority": "owner",
                "authenticated_api_credential": OWNER
            }),
        ),
        (
            "model_provider.add_from_secret",
            json!({"harness":HARNESS,"provider":provider,"source_secret":"named-vault-secret"}),
        ),
        (
            "model_provider.add",
            json!({"harness":HARNESS,"provider":provider,"value":"opaque-value"}),
        ),
        (
            "model_provider.update",
            json!({"harness":HARNESS,"provider":provider,"value":"opaque-value"}),
        ),
        (
            "model_provider.remove",
            json!({"harness":HARNESS,"name":"existing"}),
        ),
        (
            "model_provider.test",
            json!({"harness":HARNESS,"name":"existing"}),
        ),
        ("model_provider.list", json!({"harness":HARNESS})),
        (
            "model_provider.preset.add",
            json!({"harness":HARNESS,"preset":{"name":"demo","model":"endpoint/model","prompt":"opaque"}}),
        ),
        (
            "model_provider.preset.remove",
            json!({"harness":HARNESS,"name":"demo"}),
        ),
        ("model_provider.preset.list", json!({"harness":HARNESS})),
        ("model_provider.future_operation", Value::Null),
        ("rest_api.restart", json!({})),
    ]
}

struct SocketBoundary {
    client: Option<BufReader<interprocess::local_socket::Stream>>,
    server: Option<std::thread::JoinHandle<()>>,
}

impl SocketBoundary {
    fn new(svc: ServiceState, fixture: &ServiceFixture) -> Self {
        let name = format!(
            "model-authority-{}.sock",
            fixture.temp.path().file_name().unwrap().to_str().unwrap()
        );
        let listener = ListenerOptions::new()
            .name(name.clone().to_ns_name::<GenericNamespaced>().unwrap())
            .create_sync()
            .unwrap();
        let server = std::thread::spawn(move || {
            let stream = listener.accept().unwrap();
            #[cfg(windows)]
            let stream = WindowsBlockingPipe::from(stream);
            handle_connection(
                stream,
                &svc.mux,
                &svc.pty,
                &svc.notif,
                &svc.output_buf,
                &svc,
            );
        });
        let client = interprocess::local_socket::Stream::connect(
            name.to_ns_name::<GenericNamespaced>().unwrap(),
        )
        .unwrap();
        Self {
            client: Some(BufReader::new(client)),
            server: Some(server),
        }
    }

    fn request(&mut self, credential: &str, method: &str, params: Value) -> Value {
        let wire = json!({
            "jsonrpc":"2.0", "id":1, "method":method, "credential":credential,
            "protocol_version":protocol::PROTOCOL_VERSION, "params":params
        });
        let client = self.client.as_mut().unwrap();
        writeln!(client.get_mut(), "{wire}").unwrap();
        client.get_mut().flush().unwrap();
        let mut line = String::new();
        client.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }
}

impl Drop for SocketBoundary {
    fn drop(&mut self) {
        drop(self.client.take());
        self.server.take().unwrap().join().unwrap();
    }
}

#[test]
fn model_provider_socket_requires_owner_before_side_effects() {
    let (fixture, svc, tab_token, calls) = fixture();
    let config = fixture.temp.path().join("model-provider.json");
    let original = std::fs::read(&config).unwrap();
    let mut socket = SocketBoundary::new(svc, &fixture);
    for (method, params) in denied_requests() {
        let response = socket.request(&tab_token, method, params);
        assert_eq!(response["error"]["code"], -32003, "{method}: {response}");
        assert_eq!(
            response["error"]["message"],
            "RPC requires the local owner credential"
        );
        assert_eq!(calls.snapshot(), [0, 0, 0, 0], "{method}");
        assert_eq!(std::fs::read(&config).unwrap(), original);
    }
    assert_eq!(
        socket.request(OWNER, "model_provider.discover", discovery())["result"]["models"],
        json!(["fixture-model"])
    );
    assert_eq!(
        socket.request(
            OWNER,
            "model_provider.remove",
            json!({"harness":HARNESS,"name":"existing"})
        )["result"]["removed"],
        "existing"
    );
    assert_eq!(calls.snapshot(), [1, 0, 1, 1]);
    let stored: Value = serde_json::from_slice(&std::fs::read(config).unwrap()).unwrap();
    assert_eq!(stored["providers"], json!([]));
}

async fn http_request(
    router: &axum::Router,
    token: &str,
    method: &str,
    params: Value,
) -> (u16, Value) {
    let response = router.clone().oneshot(
        Request::post("/api/cli/rpc")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .extension(ConnectInfo("127.0.0.1:7000".parse::<std::net::SocketAddr>().unwrap()))
            .body(Body::from(json!({
                "method":method,"params":params,"protocol_version":protocol::PROTOCOL_VERSION
            }).to_string())).unwrap()
    ).await.unwrap();
    let status = response.status().as_u16();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[test]
fn model_provider_http_rpc_requires_owner_before_side_effects() {
    crate::register_socket();
    let (fixture, svc, tab_token, calls) = fixture();
    let config = fixture.temp.path().join("model-provider.json");
    let original = std::fs::read(&config).unwrap();
    let state = codeterm_api::test_support::app_state(&fixture);
    let router = codeterm_api::routes::create_router_with_frontend(state, None);
    codeterm_service_core::async_runtime::block_on(async {
        for (method, params) in denied_requests() {
            let (status, response) = http_request(&router, &tab_token, method, params).await;
            assert_eq!(status, 403, "{method}: {response}");
            assert_eq!(response["error"]["code"], -32003);
            assert_eq!(
                response["error"]["message"],
                "RPC requires the local owner credential"
            );
            assert_eq!(calls.snapshot(), [0, 0, 0, 0], "{method}");
            assert_eq!(std::fs::read(&config).unwrap(), original);
        }
        let (status, response) =
            http_request(&router, OWNER, "model_provider.discover", discovery()).await;
        assert_eq!(status, 200, "{response}");
        assert_eq!(response["result"]["models"], json!(["fixture-model"]));
        let (status, response) = http_request(
            &router,
            OWNER,
            "model_provider.remove",
            json!({"harness":HARNESS,"name":"existing"}),
        )
        .await;
        assert_eq!(status, 200, "{response}");
        assert_eq!(response["result"]["removed"], "existing");
    });
    assert_eq!(calls.snapshot(), [1, 0, 1, 1]);
    let stored: Value = serde_json::from_slice(&std::fs::read(config).unwrap()).unwrap();
    assert_eq!(stored["providers"], json!([]));
    assert!(svc.identify_tab_credential(&tab_token).is_some());
}
