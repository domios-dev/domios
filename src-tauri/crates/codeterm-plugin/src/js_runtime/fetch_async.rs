use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use codeterm_types::decision::{DecisionAnswer, DecisionModel, DecisionQuestion, DecisionRequest};
use serde_json::json;

use super::host_api::PluginSandbox;
use super::manifest::PermissionsConfig;
use super::{JsDecisionModel, PluginVm};

#[test]
fn fetch_async_respects_network_allow() {
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![], vec![])
            .with_identity_and_perms("fetch-async-gate", &PermissionsConfig::default()),
    );
    let vm = PluginVm::from_source_sandboxed(
        "fetch-async-gate",
        r#"module.exports.default = {
            fetch() {
                return host.fetch.async(
                    { url: "http://blocked.example.test/decision" },
                    result => result.error || String(result.status)
                );
            }
        };"#
        .into(),
        None,
        sandbox,
    )
    .expect("vm");
    let result: String = super::call_json(&vm, "fetch", &[]).expect("fetch result");
    assert_eq!(result, "fetch denied: blocked.example.test");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetch_async_releases_vm_during_request() {
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let app = Router::new().route(
        "/decision",
        get({
            let barrier = barrier.clone();
            move || {
                let barrier = barrier.clone();
                async move {
                    if tokio::time::timeout(Duration::from_secs(3), barrier.wait())
                        .await
                        .is_err()
                    {
                        return (StatusCode::GATEWAY_TIMEOUT, Json(json!({}))).into_response();
                    }
                    (StatusCode::OK, Json(json!({ "type": "noul", "p": 0.65 }))).into_response()
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("listener address");
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
    });
    let vm = Arc::new(
        PluginVm::from_source(
            "decision-fetch-concurrency",
            r#"module.exports.default = {
                primitives() { return { choice: false, noul: true, score: false }; },
                decide(request) {
                    return host.fetch.async(
                        { url: request.state.url, timeoutMs: 3000 },
                        result => {
                            if (result.error || result.status !== 200) throw new Error("fetch failed");
                            return JSON.parse(result.body);
                        }
                    );
                }
            };"#
                .into(),
        )
        .expect("vm"),
    );
    let model = Arc::new(JsDecisionModel::new(vm, "decision-fetch-concurrency"));
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    for _ in 0..2 {
        let model = model.clone();
        let result_tx = result_tx.clone();
        let request = DecisionRequest {
            state: json!({ "url": format!("http://{address}/decision") }),
            instructions: "parallel fixture".into(),
            question: DecisionQuestion::Noul { criteria: None },
        };
        std::thread::spawn(move || {
            let result = model.decide(&request, Instant::now() + Duration::from_secs(7));
            let _ = result_tx.send(result);
        });
    }
    drop(result_tx);

    let first = result_rx.recv_timeout(Duration::from_secs(8));
    let second = result_rx.recv_timeout(Duration::from_secs(8));
    let _ = shutdown_tx.send(());
    let _ = server.await;

    assert_eq!(
        first.expect("first decision"),
        Ok(DecisionAnswer::Noul { p: 0.65 })
    );
    assert_eq!(
        second.expect("second decision"),
        Ok(DecisionAnswer::Noul { p: 0.65 })
    );
}
