use super::*;
use codeterm_types::native_update::apply::NativeUpdateAdmissionRefusal as Refusal;
use codeterm_types::native_update::{ExpectedServer, NativeUpdateAction, NativeUpdateOperationId};
use codeterm_types::server_bootstrap::{ClientIdentity, ServerExpectation};
use uuid::Uuid;

struct DetachedStop;

#[async_trait::async_trait]
impl ServerStop for DetachedStop {
    async fn stop_server(&self) -> Result<(), ServerStopError> {
        unreachable!("detached preparation must not stop the server")
    }
}

#[tokio::test]
async fn detached_preparation_does_not_report_relaunching_before_helper_readiness() {
    let root = tempfile::tempdir().unwrap();
    let operation = Arc::new(UpdateOperation::default());
    operation.admit(record(1, root.path()), persist).unwrap();
    let stop = UpdateServerStop {
        inner: Arc::new(DetachedStop),
        operation: operation.clone(),
    };
    stop.prepare_detached_update("1.18.6").await.unwrap();
    let current = operation.record.lock();
    let current = current.as_ref().unwrap();
    assert_eq!(current.result.phase, Phase::Applying);
    assert_eq!(current.target_version.as_deref(), Some("1.18.6"));
}

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
        Err(NativeHostRejection::UpdateAdmission {
            reason: Refusal::OperationConflict
        })
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
    assert_eq!(encoded["replay"]["used"].as_array().unwrap().len(), 32);
    let restored = UpdateOperation::default();
    *restored.record.lock() = Some(persisted);
    assert_eq!(
        restored.admit(record(1, root.path()), |_| panic!(
            "evicted A cannot execute"
        )),
        Err(NativeHostRejection::UpdateAdmission {
            reason: Refusal::RetiredOperation
        })
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

#[test]
fn exact_incarnation_cap_refuses_without_writes_and_restart_reopens_admission() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let root = tempfile::tempdir().unwrap();
    let operation = UpdateOperation::default();
    let writes = AtomicUsize::new(0);
    let admitted = AtomicUsize::new(0);
    let cap = codeterm_update::native_handoff::replay::MAX_INCARNATION_ADMISSIONS;
    for number in 1..=cap {
        let (is_new, _) = operation
            .admit(record(number as u128, root.path()), |_| {
                writes.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .unwrap();
        assert!(is_new);
        admitted.fetch_add(usize::from(is_new), Ordering::SeqCst);
        operation
            .record
            .lock()
            .as_mut()
            .unwrap()
            .advance(Phase::NoUpdate)
            .unwrap();
    }
    assert_eq!(writes.load(Ordering::SeqCst), cap);
    let before = operation.record.lock().clone();
    assert_eq!(
        operation.admit(record((cap + 1) as u128, root.path()), |_| panic!(
            "refusal must not write or execute"
        )),
        Err(NativeHostRejection::UpdateAdmission {
            reason: Refusal::IncarnationLimit
        })
    );
    assert_eq!(
        operation.admit(record(1, root.path()), |_| panic!(
            "evicted ID cannot execute"
        )),
        Err(NativeHostRejection::UpdateAdmission {
            reason: Refusal::RetiredOperation
        })
    );
    let mut conflict = record(cap as u128, root.path());
    conflict.options.initiating_tab_id = Some("different".into());
    assert_eq!(
        operation.admit(conflict, |_| panic!("conflict cannot execute")),
        Err(NativeHostRejection::UpdateAdmission {
            reason: Refusal::OperationConflict
        })
    );
    assert!(
        !operation
            .admit(record(cap as u128, root.path()), |_| panic!(
                "known ID cannot execute"
            ))
            .unwrap()
            .0
    );
    assert_eq!(*operation.record.lock(), before);
    assert_eq!(writes.load(Ordering::SeqCst), cap);
    assert_eq!(admitted.load(Ordering::SeqCst), cap);

    let mut restarted = record((cap + 1) as u128, root.path());
    restarted.request.expected_server.incarnation.epoch = "replacement-epoch".into();
    restarted.request.expected_server.incarnation.process.pid = 2;
    restarted
        .request
        .expected_server
        .incarnation
        .process
        .created = "replacement-created".into();
    restarted.result.connection.server_epoch = "replacement-epoch".into();
    assert!(
        operation
            .admit(restarted, |_| {
                writes.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .unwrap()
            .0
    );
    let after = serde_json::to_value(operation.record.lock().as_ref().unwrap()).unwrap();
    assert_eq!(after["replay"]["used"].as_array().unwrap().len(), 1);
    assert_eq!(after["replay"]["recent"].as_array().unwrap().len(), 16);
    assert_eq!(writes.load(Ordering::SeqCst), cap + 1);
}
