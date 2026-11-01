use super::*;
use codeterm_types::native_update::{ExpectedServer, NativeUpdateAction, NativeUpdateOperationId};
use codeterm_types::server_bootstrap::{ClientIdentity, ServerExpectation};
use uuid::Uuid;

fn record(number: u128, data: &std::path::Path) -> NativeUpdateHandoff {
    let components = ServerComponentManifest {
        schema: 2,
        server_build: "fixture".into(),
        server_executable: "server".into(),
        server_sha256: "a".repeat(64),
        client_identity: ClientIdentity::Sha256 {
            sha256: "b".repeat(64),
        },
        api_version: 3,
        bootstrap_protocol: 2,
    };
    let identity = codeterm_types::server_bootstrap::ServerIdentity {
        data: data.into(),
        config: data.join("config.toml"),
    };
    NativeUpdateHandoff::new(
        NativeUpdateRequest {
            operation_id: NativeUpdateOperationId::new(Uuid::from_u128(number)).unwrap(),
            action: NativeUpdateAction::Pair,
            expected_server: ExpectedServer {
                components: components.clone(),
                identity: identity.clone(),
                incarnation: ServerExpectation {
                    process: ProcessProvenance {
                        pid: 1,
                        created: "fixture".into(),
                        executable: "server".into(),
                    },
                    epoch: "epoch".into(),
                },
            },
            staged_components: components,
            restart_authorized: true,
        },
        NativeUpdateApplyOptions {
            channel: codeterm_update::Channel::Stable,
            manifest_url: None,
            initiating_tab_id: None,
        },
        NativeHostConnection {
            instance_id: "fixture".into(),
            identity,
            server_epoch: "epoch".into(),
            generation: 1,
        },
    )
}

#[test]
fn terminal_a_then_b_then_a_returns_persisted_result_without_admitting_execution() {
    let root = tempfile::tempdir().unwrap();
    let operation = UpdateOperation::default();
    let a = record(1, root.path());
    let b = record(2, root.path());
    assert!(operation.admit(a.clone(), persist).unwrap().0);
    operation
        .mutate(|record| record.advance(Phase::NoUpdate))
        .unwrap();
    let a_result = operation.record.lock().as_ref().unwrap().result.clone();
    assert!(operation.admit(b, persist).unwrap().0);
    operation
        .mutate(|record| {
            record.advance(Phase::Failed {
                message: "B failed".into(),
            })
        })
        .unwrap();
    let persisted = codeterm_update::native_handoff::read(root.path())
        .unwrap()
        .unwrap();
    let restored = UpdateOperation::default();
    *restored.record.lock() = Some(persisted.clone());
    let (admitted, result) = restored
        .admit(a.clone(), |_| {
            panic!("A must not be persisted or executed again")
        })
        .unwrap();
    assert!(!admitted);
    assert_eq!(result, a_result);
    assert_eq!(
        codeterm_update::native_handoff::read(root.path()).unwrap(),
        Some(persisted)
    );
    let mut changed = a;
    changed.options.initiating_tab_id = Some("different".into());
    assert_eq!(
        restored.admit(changed, |_| panic!("conflict cannot execute")),
        Err(NativeHostRejection::ProtocolMismatch)
    );
}

#[test]
fn evicted_operation_ids_remain_refused_after_restart_with_bounded_evidence() {
    let root = tempfile::tempdir().unwrap();
    let operation = UpdateOperation::default();
    for number in 1..=32 {
        assert!(
            operation
                .admit(record(number, root.path()), persist)
                .unwrap()
                .0
        );
        operation
            .mutate(|record| record.advance(Phase::NoUpdate))
            .unwrap();
    }
    let persisted = codeterm_update::native_handoff::read(root.path())
        .unwrap()
        .unwrap();
    let encoded = serde_json::to_value(&persisted).unwrap();
    assert_eq!(encoded["replay"]["recent"].as_array().unwrap().len(), 16);
    assert_eq!(encoded["replay"]["used"].as_array().unwrap().len(), 1024);
    let restored = UpdateOperation::default();
    *restored.record.lock() = Some(persisted);
    assert_eq!(
        restored.admit(record(1, root.path()), |_| panic!(
            "evicted A cannot execute"
        )),
        Err(NativeHostRejection::ProtocolMismatch)
    );
}

#[test]
fn failed_admission_write_does_not_forget_predecessor_or_mark_new_id_used() {
    let root = tempfile::tempdir().unwrap();
    let operation = UpdateOperation::default();
    operation.admit(record(1, root.path()), persist).unwrap();
    operation
        .mutate(|record| record.advance(Phase::NoUpdate))
        .unwrap();
    let before = operation.record.lock().clone();
    assert_eq!(
        operation.admit(record(2, root.path()), |_| Err(
            "fixture write refusal".into()
        )),
        Err(NativeHostRejection::HostRejected)
    );
    assert_eq!(*operation.record.lock(), before);
    assert_eq!(
        operation.recorded_result(
            &record(2, root.path()).request,
            &record(2, root.path()).options
        ),
        Ok(None)
    );
}
