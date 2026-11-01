use super::*;
use codeterm_types::{
    native_host::NativeHostConnection,
    native_update::{
        ExpectedServer, NativeUpdateAction, NativeUpdateOperationId, NativeUpdateRefusal,
    },
    server_bootstrap::{
        ProcessProvenance, ServerComponentManifest, ServerExpectation, ServerIdentity, API_VERSION,
        BOOTSTRAP_PROTOCOL, SERVER_COMPONENT_SCHEMA,
    },
    update::channel::Channel,
};
use uuid::Uuid;

fn components(client_digest: &str) -> ServerComponentManifest {
    ServerComponentManifest {
        schema: SERVER_COMPONENT_SCHEMA,
        server_build: "server-old".into(),
        server_executable: PathBuf::from(if cfg!(target_os = "macos") {
            "../MacOS/domios-server"
        } else if cfg!(windows) {
            "domios-server/domios-server.exe"
        } else {
            "domios-server/domios-server"
        }),
        server_sha256: "a".repeat(64),
        client_identity: if cfg!(target_os = "macos") {
            codeterm_types::server_bootstrap::ClientIdentity::MacosDesignatedRequirement {
                requirement: r#"anchor apple generic and identifier "com.domios.app" and certificate leaf[subject.OU] = "ABCDE12345""#.into(),
                team_id: "ABCDE12345".into(), identifier: "com.domios.app".into(),
            }
        } else {
            codeterm_types::server_bootstrap::ClientIdentity::Sha256 {
                sha256: client_digest.into(),
            }
        },
        api_version: API_VERSION,
        bootstrap_protocol: BOOTSTRAP_PROTOCOL,
    }
}

fn resource_root() -> PathBuf {
    PathBuf::from(if cfg!(target_os = "macos") {
        "/Applications/Domios.app/Contents/Resources"
    } else if cfg!(windows) {
        "C:/Domios/resources"
    } else {
        "/domios/resources"
    })
}

fn request() -> NativeUpdateRequest {
    let current = components(&"b".repeat(64));
    let staged = components(&"c".repeat(64));
    let identity = ServerIdentity {
        data: PathBuf::from("C:/Domios/data"),
        config: PathBuf::from("C:/Domios/config"),
    };
    let process = ProcessProvenance {
        pid: 41,
        created: "start-41".into(),
        executable: if cfg!(target_os = "macos") {
            resource_root()
                .parent()
                .unwrap()
                .join("MacOS/domios-server")
        } else {
            resource_root().join(&current.server_executable)
        },
    };
    NativeUpdateRequest {
        operation_id: NativeUpdateOperationId::new(Uuid::from_u128(17)).unwrap(),
        action: NativeUpdateAction::Pair,
        expected_server: ExpectedServer {
            components: current,
            identity,
            incarnation: ServerExpectation {
                process,
                epoch: "epoch-old".into(),
            },
        },
        staged_components: staged,
        restart_authorized: true,
    }
}

fn ready_for(request: &NativeUpdateRequest) -> ServerReady {
    let expected = &request.expected_server;
    ServerReady {
        protocol: expected.components.bootstrap_protocol,
        api_version: expected.components.api_version,
        build: expected.components.server_build.clone(),
        instance_id: "instance".into(),
        identity: expected.identity.clone(),
        epoch: expected.incarnation.epoch.clone(),
        process: expected.incarnation.process.clone(),
        api_address: "127.0.0.1:7686".parse().expect("valid test address"),
        socket_address: "socket".into(),
        bootstrap_address: "bootstrap".into(),
        resources: resource_root(),
    }
}

fn handoff() -> NativeUpdateHandoff {
    let request = request();
    let connection = NativeHostConnection {
        instance_id: "instance".into(),
        identity: request.expected_server.identity.clone(),
        server_epoch: "epoch-old".into(),
        generation: 7,
    };
    NativeUpdateHandoff::new(
        request,
        NativeUpdateApplyOptions {
            channel: Channel::Stable,
            manifest_url: None,
            initiating_tab_id: Some("caller-tab".into()),
        },
        connection,
    )
}

fn replacement(record: &NativeUpdateHandoff) -> (NativeHostConnection, ServerReady) {
    let mut ready = ready_for(&record.request);
    ready.epoch = "replacement-epoch".into();
    ready.process.pid = 73;
    ready.process.created = "replacement-start".into();
    ready.build = "replacement-build".into();
    let mut connection = record.result.connection.clone();
    connection.server_epoch = ready.epoch.clone();
    connection.generation = 1;
    (connection, ready)
}

#[test]
fn native_pair_decision_requires_restart_authority_and_exact_current_server() {
    let mut request = request();
    let ready = ready_for(&request);
    let manifest = request.expected_server.components.clone();
    assert_eq!(validate_apply(&request, &ready, &manifest), Ok(()));
    request.restart_authorized = false;
    assert_eq!(
        validate_apply(&request, &ready, &manifest),
        Err(NativeUpdateRefusal::RestartAuthorizationRequired)
    );
    request.restart_authorized = true;
    let mut foreign = ready;
    foreign.epoch = "another-incarnation".into();
    assert_eq!(
        validate_apply(&request, &foreign, &manifest),
        Err(NativeUpdateRefusal::ExpectedServerMismatch)
    );
    request.action = NativeUpdateAction::ClientOnly;
    request.restart_authorized = false;
    assert_eq!(
        validate_apply(&request, &foreign, &manifest),
        Err(NativeUpdateRefusal::DurableHandoffUnavailable)
    );
}

#[test]
fn package_managed_methods_never_select_native_apply() {
    use crate::InstallMethod::*;
    for method in [Brew, Scoop, Rpm, Unknown] {
        assert!(!supports_native_apply(&method));
    }
    for method in [Dmg, Nsis, Msi, AppImage, Deb] {
        assert!(supports_native_apply(&method));
    }
}

#[test]
fn handoff_round_trip_keeps_caller_target_operation_and_sequence_across_restart() {
    let mut record = handoff();
    record.target_version = Some("1.15.2".into());
    record.advance(NativeUpdateApplyPhase::Relaunching).unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(path(root.path()), serde_json::to_vec(&record).unwrap()).unwrap();
    let mut restored = read(root.path()).unwrap().unwrap();
    assert_eq!(restored, record);
    let (connection, ready) = replacement(&restored);
    assert_eq!(
        restored.reconcile("1.15.2", "replacement-build", &connection, &ready),
        Ok(true)
    );
    assert_eq!(restored.result.operation_id.value(), Uuid::from_u128(17));
    assert_eq!(
        restored.options.initiating_tab_id.as_deref(),
        Some("caller-tab")
    );
    assert_eq!(restored.result.sequence, 3);
    assert_eq!(
        restored.result.phase,
        NativeUpdateApplyPhase::Verified {
            version: "1.15.2".into(),
            server: Box::new(ready),
        }
    );
    assert_eq!(restored.result.connection, connection);
    assert_eq!(
        restored.reconcile(
            "1.15.2",
            "replacement-build",
            &connection,
            &replacement(&record).1
        ),
        Ok(false)
    );
}

#[test]
fn installed_version_mismatch_is_failure_and_never_reexecutes_the_installer() {
    let mut record = handoff();
    record.target_version = Some("1.15.2".into());
    record.advance(NativeUpdateApplyPhase::Relaunching).unwrap();
    let (connection, ready) = replacement(&record);
    assert_eq!(
        record.reconcile("1.15.1", "replacement-build", &connection, &ready),
        Ok(true)
    );
    assert!(matches!(
        record.result.phase,
        NativeUpdateApplyPhase::Failed { .. }
    ));
    assert_eq!(
        record.reconcile("1.15.2", "replacement-build", &connection, &ready),
        Ok(false)
    );
}

#[test]
fn predecessor_and_foreign_instance_never_verify_a_pair_handoff() {
    let mut record = handoff();
    record.target_version = Some("1.15.2".into());
    record.advance(NativeUpdateApplyPhase::Relaunching).unwrap();
    let prior = record.clone();
    assert!(record
        .reconcile(
            "1.15.2",
            "replacement-build",
            &prior.result.connection,
            &ready_for(&prior.request)
        )
        .is_err());
    assert_eq!(record, prior);
    let (mut connection, mut ready) = replacement(&record);
    connection.identity.data = PathBuf::from("foreign-data");
    ready.identity = connection.identity.clone();
    assert!(record
        .reconcile("1.15.2", "replacement-build", &connection, &ready)
        .is_err());
    assert_eq!(record, prior);
}

#[test]
fn missing_handoff_is_empty_and_corrupt_handoff_is_an_error() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(read(root.path()).unwrap(), None);
    std::fs::write(path(root.path()), b"{").unwrap();
    assert!(read(root.path()).is_err());
}

#[test]
fn matching_client_version_with_a_foreign_server_build_is_not_verified() {
    let mut record = handoff();
    record.target_version = Some("1.15.2".into());
    record.advance(NativeUpdateApplyPhase::Relaunching).unwrap();
    let (connection, ready) = replacement(&record);
    assert_eq!(
        record.reconcile("1.15.2", "other-client-build", &connection, &ready),
        Ok(true)
    );
    assert!(matches!(
        record.result.phase,
        NativeUpdateApplyPhase::Failed { .. }
    ));
}

#[test]
fn failed_stop_and_foreign_handoffs_become_failed_without_blocking_attach() {
    for foreign in [false, true] {
        let mut record = handoff();
        record.advance(NativeUpdateApplyPhase::Relaunching).unwrap();
        let mut ready = ready_for(&record.request);
        if foreign {
            ready.identity.data.push("foreign");
        }
        let connection = record.result.connection.clone();
        assert!(record.recover("1.15.2", "new-build", &connection, &ready));
        assert!(matches!(
            record.result.phase,
            NativeUpdateApplyPhase::Failed { .. }
        ));
    }
}

#[test]
fn corrupt_or_unreadable_handoff_is_absent_from_status_and_attach() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(path(root.path()), b"{corrupt").unwrap();
    assert!(read_optional(root.path()).is_none());
    std::fs::remove_file(path(root.path())).unwrap();
    std::fs::create_dir(path(root.path())).unwrap();
    assert!(read_optional(root.path()).is_none());
}

#[test]
fn additional_handoff_fields_do_not_break_recovery() {
    let mut value = serde_json::to_value(handoff()).unwrap();
    value["future_field"] = serde_json::json!(true);
    assert!(serde_json::from_value::<NativeUpdateHandoff>(value).is_ok());
}

#[test]
fn recovery_quarantines_corrupt_records_without_replaying_them() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(path(root.path()), b"invalid").unwrap();
    assert!(read_for_recovery(root.path()).is_none());
    assert!(!path(root.path()).exists());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    assert!(read_for_recovery(root.path()).is_none());
}
