use super::*;
use codeterm_types::native_host::{NativeHostConnection, NativeWebviewDiagnosticsStatus};
use codeterm_types::server_bootstrap::ProcessProvenance;

#[test]
fn same_build_restart_reports_current_attachment_before_native_root_locator() {
    let initial: ServerReady = serde_json::from_value(json!({
        "protocol": 2, "api_version": 3, "build": "same-build", "instance_id": "instance",
        "identity": {"data": "data", "config": "config"}, "epoch": "old-epoch",
        "process": {"pid": 42, "created": "123", "executable": "server"},
        "api_address": "127.0.0.1:7887", "socket_address": "socket",
        "bootstrap_address": "bootstrap", "resources": "resources"
    }))
    .unwrap();
    let mut replacement = initial.clone();
    replacement.epoch = "new-epoch".into();
    replacement.process.pid = 43;
    let client_process = ProcessProvenance {
        pid: 100,
        created: "456".into(),
        executable: "client".into(),
    };
    let snapshot = NativeWebviewDiagnosticsSnapshot {
        connection: NativeHostConnection {
            instance_id: replacement.instance_id.clone(),
            identity: replacement.identity.clone(),
            server_epoch: replacement.epoch.clone(),
            generation: 1,
        },
        view_session: uuid::Uuid::nil(),
        client_process: client_process.clone(),
        status: NativeWebviewDiagnosticsStatus::Disabled,
    };

    let initial_receipt = attachment_receipt(&initial, client_process.pid);
    let recovery_receipts = root_receipts(
        &replacement,
        &snapshot,
        2,
        Value::Null,
        "http://127.0.0.1:7887",
    );
    let (attachment, locator) = recovery_receipts.split_once('\n').unwrap();
    assert_ne!(initial_receipt, attachment);
    let attachment: Value =
        serde_json::from_str(attachment.strip_prefix("DOMIOS_DEV_ATTACHMENT ").unwrap()).unwrap();
    let locator: Value =
        serde_json::from_str(locator.strip_prefix("DOMIOS_DEV_NATIVE_LOCATOR ").unwrap()).unwrap();
    assert_eq!(attachment["server_pid"], replacement.process.pid);
    assert_eq!(attachment["epoch"], replacement.epoch);
    assert_eq!(attachment["api_address"], initial.api_address.to_string());
    assert_eq!(attachment["client_pid"], client_process.pid);
    assert_eq!(
        locator["snapshot"]["connection"]["server_epoch"],
        attachment["epoch"]
    );
    assert_eq!(
        locator["snapshot"]["client_process"]["pid"],
        attachment["client_pid"]
    );
    assert_eq!(locator["navigation_generation"], 2);
}
