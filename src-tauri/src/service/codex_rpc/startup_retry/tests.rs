use super::*;

#[tokio::test]
async fn codex_rpc_startup_retry_transient_then_success_uses_sidecar() {
    let mut attempts = 0;
    let result = run("retry-success", || {
        attempts += 1;
        std::future::ready(if attempts == 1 {
            Err("Sidecar listening deadline".into())
        } else {
            Ok("sidecar-command")
        })
    })
    .await;
    assert_eq!(result.unwrap(), "sidecar-command");
    assert_eq!(attempts, 2);
}

#[tokio::test]
async fn codex_rpc_startup_retry_deterministic_refusal_is_attempted_once() {
    for error in [
        "Sidecar thread identity unverified",
        "Unsupported app-server protocol version",
        "Existing sidecar owns another thread; explicit restart required",
        "Sidecar cwd invalid: os error 123",
        "App-server request failed: Rpc(permission denied)",
        "Sidecar cleanup deadline",
        "Sidecar exited before listening",
        "RPC WebSocket handshake refused: HTTP 403",
    ] {
        let mut attempts = 0;
        let result: Result<(), String> = run("retry-refused", || {
            attempts += 1;
            std::future::ready(Err(error.into()))
        })
        .await;
        assert_eq!(result.unwrap_err(), error);
        assert_eq!(attempts, 1);
    }
}

#[tokio::test]
async fn codex_rpc_startup_retry_exhaustion_returns_one_fallback() {
    let mut attempts = 0;
    let mut fallbacks = 0;
    let result: Result<&str, String> = run("retry-exhausted", || {
        attempts += 1;
        std::future::ready(Err("RPC WebSocket connection failed".into()))
    })
    .await;
    let command = result.unwrap_or_else(|_| {
        fallbacks += 1;
        "plain-pty"
    });
    assert_eq!(command, "plain-pty");
    assert_eq!(attempts, MAX_ATTEMPTS);
    assert_eq!(fallbacks, 1);
}

#[test]
fn codex_rpc_startup_retry_only_transport_failures_are_transient() {
    assert!(transient("App-server request failed: Deadline"));
    assert!(transient(
        "App-server request failed: Closed(\"RPC reader exited\")"
    ));
    assert!(!transient(
        "Thread identity unverified: RPC WebSocket connection failed"
    ));
    assert!(!transient(
        "App-server request failed: Closed(\"Invalid RPC frame\")"
    ));
}

#[tokio::test]
async fn codex_rpc_startup_retry_superseded_launch_never_starts_another_server() {
    let mut nonce = "original";
    let mut servers = 0;
    let result: Result<(), String> = run("retry-superseded", || {
        let result = verify_launch("original", Some(nonce)).and_then(|_| {
            servers += 1;
            nonce = "replacement";
            Err("Sidecar listening deadline".into())
        });
        std::future::ready(result)
    })
    .await;
    assert_eq!(result.unwrap_err(), SUPERSEDED);
    assert_eq!(servers, 1);
    assert!(!transient(SUPERSEDED));
    assert!(verify_launch("original", None).is_err());
}
