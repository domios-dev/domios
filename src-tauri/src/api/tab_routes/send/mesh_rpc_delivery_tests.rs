use super::{route_send_origin, run_programmatic_delivery, SendTextBody};
use crate::multiplexer::state::MultiplexerState;
use crate::service::codex_rpc::send::route_or_pty;
use crate::service::delivery_error::{DeliveryError, SendOrigin};
use axum::http::StatusCode;
use parking_lot::Mutex;
use std::sync::Arc;

#[tokio::test]
async fn local_and_mesh_rpc_delivery_resolve_target_and_write_once() {
    for peer in [None, Some("authenticated-peer")] {
        let body: SendTextBody = serde_json::from_value(serde_json::json!({
            "text": "opaque payload", "source_tab": "sender", "source_peer": peer
        }))
        .unwrap();
        assert_eq!(
            route_send_origin(false, body.source_tab.is_some(), body.source_peer.is_some()),
            SendOrigin::Programmatic
        );
        let (mut mux, tab) = MultiplexerState::new();
        mux.get_tab_mut(&tab).unwrap().agent_type = Some("opaque-rpc-provider".into());
        let target = tab.clone();
        let writes = Arc::new(Mutex::new(Vec::new()));
        let captured = writes.clone();
        let result = run_programmatic_delivery(&tab, move || {
            let provider = mux.get_tab(&target).unwrap().agent_type.clone().unwrap();
            route_or_pty(
                Some(tauri::async_runtime::block_on(async move {
                    captured.lock().push((provider, body.text.unwrap()));
                    Ok::<_, String>(())
                })),
                || panic!("RPC selection must not fall back to PTY"),
            )
        })
        .await;
        assert!(result.is_ok(), "RPC delivery failed");
        assert_eq!(
            *writes.lock(),
            [(
                "opaque-rpc-provider".to_string(),
                "opaque payload".to_string()
            )]
        );
    }
}

#[tokio::test]
async fn resolution_failure_returns_typed_zero_write_refusal() {
    let calls = Arc::new(Mutex::new(0));
    let captured = calls.clone();
    let result = run_programmatic_delivery("target", move || {
        *captured.lock() += 1;
        Err::<(), _>(
            DeliveryError::TabBusyRefused {
                tab: "target".into(),
                detail: "provider resolution unavailable".into(),
            }
            .ledger_message(),
        )
    })
    .await;
    let rejection = result.expect_err("resolution must fail");
    assert_eq!(rejection.status, StatusCode::CONFLICT);
    assert_eq!(rejection.body["error"]["code"], "tab_busy_refused");
    assert_eq!(rejection.body["error"]["bytes_written"], false);
    assert_eq!(*calls.lock(), 1);
}

#[tokio::test]
async fn delivery_worker_panic_is_typed_uncertainty_without_replay() {
    let writes = Arc::new(Mutex::new(0));
    let captured = writes.clone();
    let result = run_programmatic_delivery("target", move || -> Result<(), String> {
        *captured.lock() += 1;
        panic!("delivery lost its response after handoff");
    })
    .await;
    let rejection = result.expect_err("panic must be classified");
    assert_eq!(rejection.status, StatusCode::BAD_GATEWAY);
    assert_eq!(rejection.body["error"]["code"], "submit_unconfirmed");
    assert_eq!(rejection.body["error"]["bytes_written"], true);
    assert_eq!(*writes.lock(), 1);
}
