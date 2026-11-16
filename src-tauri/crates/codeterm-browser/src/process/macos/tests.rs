use super::*;
use std::process::Stdio;

const ID: &str = "5c1a3b9e-8f0d-4f8e-9f41-2b1c0e6d7a10";

#[test]
fn browser_macos_endpoint_must_identify_the_launched_browser() {
    for info in [
        json!({}),
        json!({"processInfo": [{"type": "renderer", "id": 42}]}),
        json!({"processInfo": [{"type": "browser", "id": 43}]}),
        json!({"processInfo": [{"type": "browser", "id": "42"}]}),
    ] {
        assert!(!browser_pid_matches(&info, 42));
    }
    assert!(browser_pid_matches(
        &json!({"processInfo": [
            {"type": "renderer", "id": 43}, {"type": "browser", "id": 42}
        ]}),
        42
    ));
}

#[tokio::test]
async fn browser_macos_observes_exact_process_exit_without_workspace_notifications() {
    let mut child = tokio::process::Command::new("/bin/sh")
        .args(["-c", "read line"])
        .stdin(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    let queue = events::process_exit(pid).unwrap();
    let events = events::ExitEvents::register(queue).unwrap();
    drop(child.stdin.take());
    tokio::time::timeout(Duration::from_secs(5), events.exited(pid))
        .await
        .unwrap()
        .unwrap();
    child.wait().await.unwrap();
}

#[tokio::test]
async fn browser_macos_port_file_watches_new_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let port = dir.path().join("DevToolsActivePort");
    std::fs::write(&port, format!("3000\n/devtools/browser/{ID}")).unwrap();
    let watcher = events::PortFile::watch(dir.path()).unwrap();
    let cancel = watcher.cancellation().unwrap();
    let result = tokio::task::spawn_blocking(move || {
        watcher.endpoint(std::time::Instant::now() + Duration::from_secs(5))
    });
    tokio::fs::write(&port, format!("4000\n/devtools/browser/{ID}"))
        .await
        .unwrap();
    assert_eq!(
        result.await.unwrap().unwrap(),
        format!("ws://127.0.0.1:4000/devtools/browser/{ID}")
    );
    drop(cancel);
}

#[tokio::test]
async fn browser_macos_port_file_rejects_stale_endpoint_and_cancels() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("DevToolsActivePort"),
        format!("3000\n/devtools/browser/{ID}"),
    )
    .unwrap();
    let watcher = events::PortFile::watch(dir.path()).unwrap();
    let cancel = watcher.cancellation().unwrap();
    drop(cancel);
    let result = tokio::task::spawn_blocking(move || {
        watcher.endpoint(std::time::Instant::now() + Duration::from_secs(5))
    });
    assert!(result.await.unwrap().unwrap_err().contains("cancelled"));
}
