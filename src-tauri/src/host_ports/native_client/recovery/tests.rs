use super::*;
use codeterm_types::server_bootstrap::{ClientIdentity, ServerComponentManifest};

#[test]
fn server_swap_preserves_client_and_protocol_identity() {
    let previous = ServerComponentManifest {
        schema: 2,
        server_build: "old".into(),
        server_executable: "domios-server/domios-server".into(),
        server_sha256: "a".repeat(64),
        client_identity: ClientIdentity::Sha256 {
            sha256: "b".repeat(64),
        },
        api_version: 3,
        bootstrap_protocol: 2,
    };
    let mut current = previous.clone();
    current.server_build = "new".into();
    current.server_sha256 = "c".repeat(64);
    assert!(same_client(&previous, &current));
    current.client_identity = ClientIdentity::Sha256 {
        sha256: "d".repeat(64),
    };
    assert!(!same_client(&previous, &current));
    current = previous.clone();
    current.bootstrap_protocol += 1;
    assert!(!same_client(&previous, &current));
    current = previous.clone();
    current.server_executable = "foreign/server".into();
    assert!(!same_client(&previous, &current));
}

#[test]
fn stale_disconnect_cannot_clear_a_replacement_epoch() {
    let ready: codeterm_types::server_bootstrap::ServerReady =
        serde_json::from_value(serde_json::json!({
            "protocol": 2, "api_version": 3, "build": "opaque", "instance_id": "instance",
            "identity": {"data": "data", "config": "config"}, "epoch": "old",
            "process": {"pid": 42, "created": "123", "executable": "server"},
            "api_address": "127.0.0.1:7887", "socket_address": "socket",
            "bootstrap_address": "bootstrap", "resources": "resources"
        }))
        .unwrap();
    let slot = crate::host_ports::ServerApiAttachmentSlot::default();
    slot.install(ready.clone(), "opaque-token".into()).unwrap();
    let mut replacement = ready.clone();
    replacement.epoch = "new".into();
    slot.install(replacement.clone(), "opaque-token".into())
        .unwrap();
    assert!(!slot.clear_if_current(&ready));
    assert_eq!(slot.current().unwrap().ready(), &replacement);
    assert!(slot.clear_if_current(&replacement));
    assert!(slot.current().is_none());
}
