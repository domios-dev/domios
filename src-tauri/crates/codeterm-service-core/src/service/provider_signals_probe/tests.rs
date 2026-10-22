use super::capture;
use codeterm_plugin::provider::AuthStatusProbe;

#[test]
fn capture_probes_a_binary_no_provider_listing_has_cached() {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(windows)]
    let (bin_path, body) = (
        dir.path().join("auth-status.cmd"),
        "@echo {\"signedIn\":true}\r\n",
    );
    #[cfg(not(windows))]
    let (bin_path, body) = (
        dir.path().join("auth-status"),
        "#!/bin/sh\necho '{\"signedIn\":true}'\n",
    );
    std::fs::write(&bin_path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&bin_path).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&bin_path, perm).unwrap();
    }
    let bin = bin_path.to_string_lossy().into_owned();
    codeterm_plugin::js_runtime::forget_resolved_bin(&bin);
    let probe = AuthStatusProbe {
        args: Vec::new(),
        timeout_ms: 5000,
    };

    let captured = capture(&bin, &probe);
    codeterm_plugin::js_runtime::forget_resolved_bin(&bin);

    assert_eq!(captured, Some(serde_json::json!({ "signedIn": true })));
}

#[tokio::test(start_paused = true)]
async fn auth_probe_accepts_real_cli_latency_and_preserves_timeout_cause() {
    use super::{await_probe, probe_timeout, ProbeFailure};
    use std::time::Duration;
    assert_eq!(probe_timeout(15_000), Duration::from_secs(15));
    assert_eq!(probe_timeout(u64::MAX), Duration::from_secs(30));
    let (send, receive) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(await_probe(15_000, async move { receive.await.ok() }));
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(7700)).await;
    send.send(serde_json::json!({"loggedIn":true})).unwrap();
    assert_eq!(
        task.await.unwrap(),
        Ok(serde_json::json!({"loggedIn":true}))
    );
    assert_eq!(
        await_probe(15_000, std::future::pending()).await,
        Err(ProbeFailure::TimedOut)
    );
    assert_eq!(
        await_probe(15_000, async { None }).await,
        Err(ProbeFailure::Unavailable)
    );
}
