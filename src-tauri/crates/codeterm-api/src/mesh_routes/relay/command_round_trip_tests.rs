use super::*;

#[test]
fn mesh_exec_command_survives_peer_envelopes_and_loopback_deserialization() {
    let command = r#"Write-Output '(?ms)^\[api\]\r?\n.*?(?=^\[|\z) .\build.ps1 C:\new\test \'"#;
    let origin: PeerRelayCommand = serde_json::from_value(serde_json::json!({
        "target_peer_id": "ROG", "method": "POST", "path": "/api/tabs/target/exec",
        "body": {"command": command, "timeout_ms": 15000, "strip_ansi": true},
    }))
    .unwrap();
    let relay = codeterm_service_core::mesh::types::RelayCommand {
        method: origin.method,
        path: origin.path,
        body: origin.body,
        delivery_id: None,
    };
    let received: codeterm_service_core::mesh::types::RelayCommand =
        serde_json::from_slice(&serde_json::to_vec(&relay).unwrap()).unwrap();
    let mut body = received.body;
    tag_relay_send_source(&mut body, &received.method, &received.path, "origin");
    tag_relay_spawn_source(
        &mut body,
        &received.method,
        &received.path,
        "peer",
        "origin",
    );
    tag_relay_notification_source(&mut body, &received.method, &received.path, "origin");
    let loopback = reqwest::Client::new()
        .post("http://127.0.0.1/api/tabs/target/exec")
        .json(&body)
        .build()
        .unwrap();
    let exec: crate::tab_routes::ExecBody =
        serde_json::from_slice(loopback.body().unwrap().as_bytes().unwrap()).unwrap();
    assert_eq!(exec.command.as_bytes(), command.as_bytes());
}
