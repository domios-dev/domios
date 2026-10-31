use super::*;
use clap::Parser;

struct AdmissionRpc(Result<serde_json::Value, String>);
impl RpcClient for AdmissionRpc {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, crate::client::SocketCallError> {
        assert_eq!(method, "cli.local.authorize");
        assert!(params.unwrap()["operation"].is_string());
        self.0
            .clone()
            .map_err(crate::client::SocketCallError::Transport)
    }
}

#[test]
fn actual_local_routes_require_admission_but_service_commands_and_offline_host_launch_are_preserved(
) {
    for (args, operation) in [
        (vec!["cli", "plugin", "list"], Some(Operation::Maintenance)),
        (
            vec!["cli", "mem", "backup", "create", "--out", "opaque.tar"],
            Some(Operation::Maintenance),
        ),
        (
            vec!["cli", "system", "api-token"],
            Some(Operation::HostOwner),
        ),
        (
            vec!["cli", "system", "supervise", "--once"],
            Some(Operation::HostOwner),
        ),
        (vec!["cli", "identify", "--json"], None),
        (vec!["cli", "docs", "NEIGHBORS"], None),
        (vec!["cli", "account", "neighbors", "status"], None),
    ] {
        let cli = crate::Cli::try_parse_from(args).unwrap();
        assert_eq!(operation_for(&cli.command, true), operation);
    }
    for args in [
        vec!["cli", "system", "supervise", "--once"],
        vec!["cli", "system", "autostart-status"],
    ] {
        let cli = crate::Cli::try_parse_from(args).unwrap();
        assert_eq!(
            operation_for(&cli.command, false),
            Some(Operation::HostOwner)
        );
        assert!(admission_required(false, true, false));
        assert!(admission_required(false, false, true));
        assert!(!admission_required(false, false, false));
    }
}

#[test]
fn denied_unavailable_and_missing_admission_never_authorize_local_io() {
    for result in [
        Err("unavailable".into()),
        Ok(serde_json::Value::Null),
        Ok(serde_json::json!({"authorized":false})),
    ] {
        let mut rpc = AdmissionRpc(result);
        assert!(require_rpc(&mut rpc, Operation::LocalInput).is_err());
    }
    let mut rpc = AdmissionRpc(Ok(serde_json::json!({"authorized":true})));
    require_rpc(&mut rpc, Operation::LocalInput).unwrap();
}

#[test]
fn denied_file_input_never_reaches_open_even_for_nonexistent_or_private_paths() {
    let error = read_local_text_with(std::path::Path::new("/missing/private/fixture"), || {
        Err("neighbor denied".into())
    })
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), "neighbor denied");
}

#[test]
fn admitted_file_input_preserves_bytes_after_authorization() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("body.txt");
    std::fs::write(&path, "opaque body\n").unwrap();
    assert_eq!(
        read_local_text_with(&path, || Ok(())).unwrap(),
        "opaque body\n"
    );
    let denied = read_local_text_with(&path, || Err("neighbor denied".into())).unwrap_err();
    assert_eq!(denied.kind(), std::io::ErrorKind::PermissionDenied);
}

#[test]
fn scoped_mesh_and_local_installer_are_admitted_before_payload_or_process_work() {
    let mesh = crate::Cli::try_parse_from(["cli", "--mesh", "opaque-peer", "identify"]).unwrap();
    assert_eq!(operation_for_cli(&mesh, true), Some(Operation::Maintenance));
    let installer = crate::Cli::try_parse_from([
        "cli",
        "update",
        "install",
        "--from-file",
        "opaque-installer",
    ])
    .unwrap();
    assert_eq!(
        operation_for_cli(&installer, true),
        Some(Operation::Maintenance)
    );
}
