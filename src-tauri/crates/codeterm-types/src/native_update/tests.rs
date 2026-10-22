use super::{
    ExpectedServer, NativeUpdateAction, NativeUpdateAttachment, NativeUpdateExecutor,
    NativeUpdateInstallReceipt, NativeUpdateObservationOutcome, NativeUpdateOperationId,
    NativeUpdateRefusal, NativeUpdateRequest, NativeUpdateResult, NativeUpdateStage,
    NativeUpdateState, NativeUpdateUncertainty, NativeUpdateVerification,
};
use crate::{
    native_host::NativeHostConnection,
    server_bootstrap::{
        ClientIdentity, ProcessProvenance, ServerComponentManifest, ServerExpectation,
        ServerIdentity, ServerReady, API_VERSION, BOOTSTRAP_PROTOCOL, SERVER_COMPONENT_SCHEMA,
    },
};
use std::{future::Future, path::PathBuf, pin::Pin};
use uuid::Uuid;

fn components(server_build: &str, client_digest: &str) -> ServerComponentManifest {
    ServerComponentManifest {
        schema: SERVER_COMPONENT_SCHEMA,
        server_build: server_build.into(),
        server_executable: if cfg!(target_os = "macos") {
            PathBuf::from("../MacOS/domios-server")
        } else {
            PathBuf::from("bin/domios-server.exe")
        },
        server_sha256: "a".repeat(64),
        client_identity: if cfg!(target_os = "macos") {
            ClientIdentity::MacosDesignatedRequirement {
                requirement: "anchor apple generic and identifier \"com.domios.app\" and certificate leaf[subject.OU] = \"JMMWQ9LWF7\"".into(),
                team_id: "JMMWQ9LWF7".into(),
                identifier: "com.domios.app".into(),
            }
        } else {
            ClientIdentity::Sha256 {
                sha256: client_digest.into(),
            }
        },
        api_version: API_VERSION,
        bootstrap_protocol: BOOTSTRAP_PROTOCOL,
    }
}

fn request(action: NativeUpdateAction, restart_authorized: bool) -> NativeUpdateRequest {
    let current = components("server-old", "b".repeat(64).as_str());
    let staged = match action {
        NativeUpdateAction::ClientOnly => components("server-old", "c".repeat(64).as_str()),
        NativeUpdateAction::Server => components("server-new", "b".repeat(64).as_str()),
        NativeUpdateAction::Pair => components("server-new", "c".repeat(64).as_str()),
    };
    let executable = if cfg!(target_os = "macos") {
        resource_root()
            .parent()
            .unwrap()
            .join("MacOS/domios-server")
    } else {
        resource_root().join(&current.server_executable)
    };
    NativeUpdateRequest {
        operation_id: NativeUpdateOperationId::new(Uuid::from_u128(17)).unwrap(),
        action,
        expected_server: ExpectedServer {
            components: current,
            identity: ServerIdentity {
                data: PathBuf::from("C:/Domios/data"),
                config: PathBuf::from("C:/Domios/config"),
            },
            incarnation: ServerExpectation {
                process: ProcessProvenance {
                    pid: 41,
                    created: "start-41".into(),
                    executable,
                },
                epoch: "epoch-old".into(),
            },
        },
        staged_components: staged,
        restart_authorized,
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

fn resource_root() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\Domios\resources")
    }
    #[cfg(not(windows))]
    {
        if cfg!(target_os = "macos") {
            PathBuf::from("/Applications/Domios.app/Contents/Resources")
        } else {
            PathBuf::from("/opt/domios/resources")
        }
    }
}

#[test]
fn manifest_client_identity_and_server_location_are_platform_bound() {
    let mut manifest = components("server-build", &"b".repeat(64));
    assert!(super::validate_manifest(&manifest).is_ok());
    manifest.client_identity = ClientIdentity::None;
    assert!(super::validate_manifest(&manifest).is_err());
    manifest = components("server-build", &"b".repeat(64));
    if cfg!(target_os = "macos") {
        manifest.server_executable = PathBuf::from("domios-server/domios-server");
        assert!(super::validate_manifest(&manifest).is_err());
        manifest.server_executable = PathBuf::from("../MacOS/domios-server");
        manifest.client_identity = ClientIdentity::Sha256 {
            sha256: "b".repeat(64),
        };
        assert!(super::validate_manifest(&manifest).is_err());
    } else {
        manifest.server_executable = PathBuf::from("../MacOS/domios-server");
        assert!(super::validate_manifest(&manifest).is_err());
        manifest.server_executable = PathBuf::from("bin/domios-server.exe");
        manifest.client_identity = ClientIdentity::MacosDesignatedRequirement {
            requirement: "anchor apple generic and identifier \"com.domios.app\" and certificate leaf[subject.OU] = \"JMMWQ9LWF7\"".into(),
            team_id: "JMMWQ9LWF7".into(),
            identifier: "com.domios.app".into(),
        };
        assert!(super::validate_manifest(&manifest).is_err());
    }
}

fn connection_for(
    request: &NativeUpdateRequest,
    generation: u64,
    server_epoch: &str,
) -> NativeHostConnection {
    NativeHostConnection {
        instance_id: "instance".into(),
        identity: request.expected_server.identity.clone(),
        server_epoch: server_epoch.into(),
        generation,
    }
}

#[test]
fn observation_outcomes_round_trip_with_finite_tags() {
    let request = request(NativeUpdateAction::ClientOnly, false);
    let reply = NativeUpdateResult {
        operation_id: request.operation_id,
        sequence: 1,
        connection: connection_for(&request, 1, &request.expected_server.incarnation.epoch),
        state: NativeUpdateState::InProgress {
            stage: NativeUpdateStage::Applying,
        },
    };
    let outcomes = [
        (NativeUpdateObservationOutcome::Reply(reply), "reply"),
        (
            NativeUpdateObservationOutcome::NoNewEvidence,
            "no_new_evidence",
        ),
        (
            NativeUpdateObservationOutcome::Unavailable {
                reason: NativeUpdateUncertainty::InvalidReplyAfterDispatch,
            },
            "unavailable",
        ),
    ];

    for (outcome, expected_tag) in outcomes {
        let encoded = serde_json::to_value(&outcome).unwrap();
        assert_eq!(encoded["observation"], expected_tag);
        match expected_tag {
            "reply" => assert_eq!(encoded["payload"]["sequence"], 1),
            "no_new_evidence" => {
                assert!(encoded.get("payload").is_none());
                assert!(encoded.get("sequence").is_none());
            }
            "unavailable" => assert!(encoded["payload"].get("sequence").is_none()),
            _ => unreachable!(),
        }
        let decoded: NativeUpdateObservationOutcome = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, outcome);
    }
}

struct Port;

impl NativeUpdateExecutor for Port {
    fn captured_attachment(&self) -> Option<NativeUpdateAttachment> {
        let request = request(NativeUpdateAction::ClientOnly, false);
        Some(NativeUpdateAttachment {
            connection: connection_for(&request, 1, &request.expected_server.incarnation.epoch),
            client_process: ProcessProvenance {
                pid: 72,
                created: "start-72".into(),
                executable: PathBuf::from("C:/Domios/domios.exe"),
            },
        })
    }

    fn dispatch(
        &self,
        request: NativeUpdateRequest,
        attachment: NativeUpdateAttachment,
    ) -> super::NativeUpdateDispatchFuture {
        Box::pin(async move {
            super::NativeUpdateDispatchOutcome::Reply(NativeUpdateResult {
                operation_id: request.operation_id,
                sequence: 1,
                connection: attachment.connection,
                state: NativeUpdateState::Admitted,
            })
        })
    }

    fn observe(
        &self,
        operation_id: NativeUpdateOperationId,
        after_sequence: u64,
    ) -> super::NativeUpdateObservationFuture {
        Box::pin(async move {
            super::NativeUpdateObservationOutcome::Reply(NativeUpdateResult {
                operation_id,
                sequence: after_sequence + 1,
                connection: NativeHostConnection {
                    instance_id: "instance".into(),
                    identity: ServerIdentity {
                        data: PathBuf::from("C:/Domios/data"),
                        config: PathBuf::from("C:/Domios/config"),
                    },
                    server_epoch: "epoch-old".into(),
                    generation: 1,
                },
                state: NativeUpdateState::Uncertain {
                    reason: NativeUpdateUncertainty::DisconnectedAfterDispatch,
                },
            })
        })
    }
}

#[test]
fn only_server_replacement_requires_handoff_and_explicit_authorization() {
    assert!(!NativeUpdateAction::ClientOnly.requires_server_handoff());
    assert!(!NativeUpdateAction::ClientOnly.requires_restart_authorization());
    for action in [NativeUpdateAction::Server, NativeUpdateAction::Pair] {
        assert!(action.requires_server_handoff());
        assert!(action.requires_restart_authorization());
    }
}

#[test]
fn client_only_update_pins_the_same_server_artifact_and_epoch() {
    let request = request(NativeUpdateAction::ClientOnly, false);
    let ready = ready_for(&request);
    let installed = request.expected_server.components.clone();
    assert_eq!(request.validate(), Ok(()));
    assert!(request.matches_current_server(&ready, &installed));

    let mut changed = request;
    changed.staged_components.server_build = "server-new".into();
    assert_eq!(
        changed.validate(),
        Err(NativeUpdateRefusal::ClientOnlyChangesServer)
    );
}

#[test]
fn server_and_pair_updates_require_restart_authorization() {
    for action in [NativeUpdateAction::Server, NativeUpdateAction::Pair] {
        let update_request = request(action, false);
        assert_eq!(
            update_request.validate(),
            Err(NativeUpdateRefusal::RestartAuthorizationRequired)
        );
        assert_eq!(request(action, true).validate(), Ok(()));
    }
}

#[test]
fn server_only_update_preserves_client_artifact_and_protocol_pair() {
    let mut request = request(NativeUpdateAction::Server, true);
    request.staged_components.client_identity = if cfg!(target_os = "macos") {
        ClientIdentity::MacosDesignatedRequirement {
            requirement: "anchor apple generic and identifier \"com.domios.app\" and certificate leaf[subject.OU] = \"ABCDEFGHIJ\"".into(),
            team_id: "ABCDEFGHIJ".into(),
            identifier: "com.domios.app".into(),
        }
    } else {
        ClientIdentity::Sha256 {
            sha256: "c".repeat(64),
        }
    };
    assert_eq!(
        request.validate(),
        Err(NativeUpdateRefusal::ServerOnlyChangesClientOrCompatibility)
    );
}

#[test]
fn current_server_match_requires_the_trusted_manifest_and_exact_incarnation() {
    let request = request(NativeUpdateAction::ClientOnly, false);
    let ready = ready_for(&request);
    let installed = request.expected_server.components.clone();
    assert!(request.matches_current_server(&ready, &installed));

    let mut replacement = ready;
    replacement.epoch = "epoch-new".into();
    assert!(!request.matches_current_server(&replacement, &installed));

    let mut wrong_artifact = installed;
    wrong_artifact.server_build = "other-build".into();
    assert!(!request.matches_current_server(&ready_for(&request), &wrong_artifact));
}

#[test]
fn current_server_match_resolves_manifest_path_under_exact_ready_resource_root() {
    let request = request(NativeUpdateAction::ClientOnly, false);
    let installed = request.expected_server.components.clone();
    let ready = ready_for(&request);
    assert!(request.matches_current_server(&ready, &installed));

    let mut wrong_root = ready.clone();
    wrong_root.resources = resource_root().join("other");
    assert!(!request.matches_current_server(&wrong_root, &installed));

    let mut traversing_root = ready.clone();
    traversing_root.resources = resource_root().join("../outside");
    assert!(!request.matches_current_server(&traversing_root, &installed));

    let mut wrong_executable_request = request.clone();
    wrong_executable_request
        .expected_server
        .components
        .server_executable = PathBuf::from("bin/other-server.exe");
    let wrong_executable_manifest = wrong_executable_request.expected_server.components.clone();
    assert!(!wrong_executable_request.matches_current_server(&ready, &wrong_executable_manifest,));
}

#[test]
fn manifest_executable_must_stay_inside_the_resource_root() {
    let mut expected_request = request(NativeUpdateAction::ClientOnly, false);
    expected_request
        .expected_server
        .components
        .server_executable = PathBuf::from("../outside/domios-server.exe");
    assert_eq!(
        expected_request.validate(),
        Err(NativeUpdateRefusal::InvalidServerManifest)
    );

    let mut staged_request = request(NativeUpdateAction::ClientOnly, false);
    staged_request.staged_components.server_executable =
        PathBuf::from("bin/../../outside/domios-server.exe");
    assert_eq!(
        staged_request.validate(),
        Err(NativeUpdateRefusal::InvalidStagedManifest)
    );

    let mut matching_request = request(NativeUpdateAction::ClientOnly, false);
    matching_request
        .expected_server
        .components
        .server_executable = PathBuf::from("../outside/domios-server.exe");
    assert!(!matching_request.matches_current_server(
        &ready_for(&matching_request),
        &matching_request.expected_server.components,
    ));
}

#[test]
fn replacement_verification_resolves_relative_executable_against_ready_root() {
    let request = request(NativeUpdateAction::ClientOnly, false);
    let server = ready_for(&request);
    let install = NativeUpdateInstallReceipt {
        installed_components: request.staged_components.clone(),
        client_process: ProcessProvenance {
            pid: 72,
            created: "start-72".into(),
            executable: PathBuf::from("/opt/domios/domios.exe"),
        },
    };
    let result_for = |server: ServerReady| NativeUpdateResult {
        operation_id: request.operation_id,
        sequence: 1,
        connection: connection_for(&request, 2, &server.epoch),
        state: NativeUpdateState::Verified {
            install: install.clone(),
            verification: NativeUpdateVerification {
                client_process: install.client_process.clone(),
                server,
            },
        },
    };

    assert_eq!(result_for(server.clone()).validate_for(&request), Ok(()));

    let mut wrong_root = server.clone();
    wrong_root.resources = resource_root().join("other");
    assert_eq!(
        result_for(wrong_root).validate_for(&request),
        Err(NativeUpdateRefusal::InvalidStagedManifest)
    );

    let mut wrong_executable = server;
    wrong_executable.process.executable = resource_root().join("bin/other-server.exe");
    assert_eq!(
        result_for(wrong_executable).validate_for(&request),
        Err(NativeUpdateRefusal::InvalidStagedManifest)
    );
}

#[test]
fn executor_port_is_object_safe_and_waits_on_operation_sequence() {
    let port: &dyn NativeUpdateExecutor = &Port;
    let request = request(NativeUpdateAction::ClientOnly, false);
    let request_id = request.operation_id;
    let attachment = port.captured_attachment().unwrap();
    attachment.validate_for(&request).unwrap();
    let first = futures_poll_ready(port.dispatch(request, attachment));
    let super::NativeUpdateDispatchOutcome::Reply(first) = first else {
        panic!("the attached test executor replies");
    };
    assert_eq!(first.sequence, 1);
    assert!(matches!(first.state, NativeUpdateState::Admitted));

    let NativeUpdateObservationOutcome::Reply(next) =
        futures_poll_ready(port.observe(request_id, first.sequence))
    else {
        panic!("the attached test executor replies to observation");
    };
    assert_eq!(next.operation_id, request_id);
    assert_eq!(next.sequence, 2);
    assert!(matches!(
        next.state,
        NativeUpdateState::Uncertain {
            reason: NativeUpdateUncertainty::DisconnectedAfterDispatch
        }
    ));
}

fn futures_poll_ready<T>(mut future: Pin<Box<dyn Future<Output = T> + Send + '_>>) -> T {
    use std::task::{Context, Poll, Waker};
    let mut context = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("the test port resolves synchronously"),
    }
}

#[test]
fn result_fences_operation_and_completed_artifact_identity() {
    let request = request(NativeUpdateAction::ClientOnly, false);
    let wrong = NativeUpdateResult {
        operation_id: NativeUpdateOperationId::new(Uuid::from_u128(18)).unwrap(),
        sequence: 1,
        connection: connection_for(&request, 1, "epoch-old"),
        state: NativeUpdateState::Admitted,
    };
    assert_eq!(
        wrong.validate_for(&request),
        Err(NativeUpdateRefusal::WrongOperation)
    );

    let client_process = ProcessProvenance {
        pid: 72,
        created: "start-72".into(),
        executable: PathBuf::from("C:/Domios/domios.exe"),
    };
    let completed = NativeUpdateResult {
        operation_id: request.operation_id,
        sequence: 4,
        connection: connection_for(&request, 1, "epoch-old"),
        state: NativeUpdateState::Completed {
            install: super::NativeUpdateInstallReceipt {
                installed_components: request.staged_components.clone(),
                client_process: client_process.clone(),
            },
        },
    };
    assert_eq!(completed.validate_for(&request), Ok(()));

    let mut wrong_install = completed;
    if let NativeUpdateState::Completed { install } = &mut wrong_install.state {
        install.installed_components.server_build = "other-build".into();
    }
    assert_eq!(
        wrong_install.validate_for(&request),
        Err(NativeUpdateRefusal::InvalidStagedManifest)
    );
}

#[test]
fn verified_client_reconnect_requires_monotonic_generation_and_same_server_epoch() {
    let request = request(NativeUpdateAction::ClientOnly, false);
    let installed = super::NativeUpdateInstallReceipt {
        installed_components: request.staged_components.clone(),
        client_process: ProcessProvenance {
            pid: 72,
            created: "start-72".into(),
            executable: PathBuf::from("C:/Domios/domios.exe"),
        },
    };
    let previous = NativeUpdateResult {
        operation_id: request.operation_id,
        sequence: 4,
        connection: connection_for(&request, 9, "epoch-old"),
        state: NativeUpdateState::InstalledAwaitingVerification {
            install: installed.clone(),
        },
    };
    let verified = NativeUpdateResult {
        operation_id: request.operation_id,
        sequence: 5,
        connection: connection_for(&request, 10, "epoch-old"),
        state: NativeUpdateState::Verified {
            install: installed.clone(),
            verification: NativeUpdateVerification {
                client_process: installed.client_process.clone(),
                server: ready_for(&request),
            },
        },
    };
    assert_eq!(verified.validate_after(&request, Some(&previous)), Ok(()));

    let mut same_generation = verified.clone();
    same_generation.connection.generation = previous.connection.generation - 1;
    assert_eq!(
        same_generation.validate_after(&request, Some(&previous)),
        Err(NativeUpdateRefusal::StaleSequence)
    );

    let mut wrong_epoch = verified;
    wrong_epoch.connection.server_epoch = "epoch-new".into();
    assert_eq!(
        wrong_epoch.validate_for(&request),
        Err(NativeUpdateRefusal::ExpectedServerMismatch)
    );
}

#[test]
fn uncertain_after_dispatch_is_not_a_terminal_completion() {
    let request = request(NativeUpdateAction::ClientOnly, false);
    let result = NativeUpdateResult {
        operation_id: request.operation_id,
        sequence: 3,
        connection: connection_for(&request, 1, "epoch-old"),
        state: NativeUpdateState::Uncertain {
            reason: NativeUpdateUncertainty::DeadlineAfterDispatch,
        },
    };
    assert_eq!(result.validate_for(&request), Ok(()));
    assert!(matches!(result.state, NativeUpdateState::Uncertain { .. }));
}
