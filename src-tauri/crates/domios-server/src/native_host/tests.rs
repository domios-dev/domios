use super::enrollment::{enroll, verify_component, EnrollmentPolicy, VerifiedClientProcess};
use super::transport::{
    NativeHostBindError, NativeHostEndpoint, NativeHostServer, Pending, Session, SessionState,
};
use crate::bootstrap::process::PinnedProcess;
use codeterm_types::{
    native_host::{
        NativeHostChallenge, NativeHostCompletion, NativeHostConnection,
        NativeHostEnrollmentRequest, NativeHostFrame, NativeHostRejection, NativeHostReply,
        NativeHostRequestId, NativeHostResult, NativeHostScope, NativeHostUncertainty,
        NativeViewBinding, NATIVE_HOST_PROTOCOL,
    },
    native_update::{
        ExpectedServer, NativeUpdateAction, NativeUpdateObservationOutcome,
        NativeUpdateOperationId, NativeUpdateRefusal, NativeUpdateRequest, NativeUpdateResult,
        NativeUpdateStage, NativeUpdateState, NativeUpdateUncertainty,
    },
    server_bootstrap::{
        ProcessProvenance, ServerComponentManifest, ServerIdentity, ServerReady, API_VERSION,
        BOOTSTRAP_PROTOCOL, SERVER_COMPONENT_SCHEMA,
    },
};
use parking_lot::{Mutex, RwLock};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64},
        Arc,
    },
};
use tokio::sync::{mpsc, oneshot, watch};

#[cfg(not(target_os = "macos"))]
mod client_process;

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn connection(generation: u64) -> NativeHostConnection {
    NativeHostConnection {
        instance_id: "instance-a".into(),
        identity: ServerIdentity {
            data: PathBuf::from("data-a"),
            config: PathBuf::from("config-a"),
        },
        server_epoch: "epoch-a".into(),
        generation,
    }
}

fn fixture_client_identity(digest: String) -> codeterm_types::server_bootstrap::ClientIdentity {
    #[cfg(target_os = "macos")]
    {
        let _ = digest;
        codeterm_types::server_bootstrap::ClientIdentity::MacosDesignatedRequirement {
            requirement: r#"anchor apple generic and identifier "com.domios.app" and certificate leaf[subject.OU] = "ABCDE12345""#.into(),
            team_id: "ABCDE12345".into(), identifier: "com.domios.app".into(),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        codeterm_types::server_bootstrap::ClientIdentity::Sha256 { sha256: digest }
    }
}

fn server_fixture(path: PathBuf, bytes: &[u8]) -> (ServerComponentManifest, ServerReady) {
    #[cfg(target_os = "macos")]
    let path = {
        let contents = path.parent().unwrap().join("Domios.app/Contents");
        fs::create_dir_all(contents.join("MacOS")).unwrap();
        fs::create_dir_all(contents.join("Resources")).unwrap();
        contents.join("MacOS/domios-server")
    };
    fs::write(&path, bytes).unwrap();
    let executable = path.canonicalize().unwrap();
    #[cfg(target_os = "macos")]
    let resource_root = path
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Resources")
        .canonicalize()
        .unwrap();
    #[cfg(not(target_os = "macos"))]
    let resource_root = path.parent().unwrap().canonicalize().unwrap();
    let identity = connection(1).identity;
    let process = ProcessProvenance {
        pid: 42,
        created: "process-created".into(),
        executable: executable.clone(),
    };
    let manifest = ServerComponentManifest {
        schema: SERVER_COMPONENT_SCHEMA,
        server_build: "server-build-a".into(),
        server_executable: if cfg!(target_os = "macos") {
            PathBuf::from("../MacOS/domios-server")
        } else {
            path.file_name().unwrap().into()
        },
        server_sha256: digest(bytes),
        client_identity: fixture_client_identity("a".repeat(64)),
        api_version: API_VERSION,
        bootstrap_protocol: BOOTSTRAP_PROTOCOL,
    };
    let ready = ServerReady {
        protocol: BOOTSTRAP_PROTOCOL,
        api_version: API_VERSION,
        build: "server-build-a".into(),
        instance_id: "instance-a".into(),
        identity,
        epoch: "epoch-a".into(),
        process,
        api_address: "127.0.0.1:7686".parse().unwrap(),
        socket_address: "socket".into(),
        bootstrap_address: "bootstrap".into(),
        resources: resource_root,
    };
    (manifest, ready)
}

fn verified_client(policy: &EnrollmentPolicy) -> VerifiedClientProcess {
    let process = PinnedProcess::open(std::process::id()).unwrap();
    let _ = policy;
    VerifiedClientProcess::for_enrollment_fixture(process.provenance().unwrap())
}

fn policy(
    connection: NativeHostConnection,
    client_sha256: String,
    allowed_view_origins: HashSet<String>,
) -> EnrollmentPolicy {
    EnrollmentPolicy {
        challenge: NativeHostChallenge::new(connection),
        client_identity: fixture_client_identity(client_sha256),
        resources: std::env::current_dir().unwrap(),
        allowed_view_origins,
    }
}

fn request(
    connection: NativeHostConnection,
    challenge: String,
    view: Option<NativeViewBinding>,
) -> NativeHostEnrollmentRequest {
    NativeHostEnrollmentRequest {
        protocol: NATIVE_HOST_PROTOCOL,
        challenge,
        connection,
        view,
    }
}

fn session(view: Option<NativeViewBinding>) -> Arc<Session<String, String>> {
    let connection = connection(9);
    let process = PinnedProcess::open(std::process::id()).unwrap();
    let client_process = process.provenance().unwrap();
    let (control, _) = mpsc::channel(64);
    let (events, _) = mpsc::channel(64);
    let (shutdown, _) = watch::channel(false);
    Arc::new(Session {
        connection,
        client_process,
        process,
        active_generation: Arc::new(AtomicU64::new(9)),
        allowed_view_origins: HashSet::from(["http://127.0.0.1:7686".into()]),
        control,
        events,
        state: Mutex::new(SessionState {
            view,
            diagnostics: None,
            pending: HashMap::new(),
        }),
        shutdown,
        retired: AtomicBool::new(false),
    })
}

#[test]
fn native_update_executor_is_unavailable_without_an_active_enrollment() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server-fixture.bin");
    let (manifest, ready) = server_fixture(path, b"server fixture image");
    let endpoint = NativeHostEndpoint::<
        codeterm_types::native_update::NativeUpdateRequest,
        codeterm_types::native_update::NativeUpdateResult,
    > {
        active: Arc::new(RwLock::new(None)),
        active_generation: Arc::new(AtomicU64::new(0)),
        attached_once: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        next_request_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        next_event_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        ready,
        manifest,
    };

    assert!(endpoint.connection().is_none());
    assert_eq!(
        endpoint.active_native_update_executor().err(),
        Some(codeterm_types::native_update::NativeUpdateRefusal::NativeHostUnavailable)
    );
}

#[test]
fn headless_inventory_verifies_flat_server_but_never_binds_native_host() {
    let directory = tempfile::tempdir().unwrap();
    let bytes = b"headless server fixture";
    let (mut manifest, mut ready) = server_fixture(directory.path().join("initial.bin"), bytes);
    let relative = if cfg!(windows) {
        "domios-server/domios-server.exe"
    } else {
        "domios-server/domios-server"
    };
    let root = directory.path().canonicalize().unwrap();
    let executable = root.join(relative);
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, bytes).unwrap();
    manifest.client_identity = codeterm_types::server_bootstrap::ClientIdentity::None;
    manifest.server_executable = relative.into();
    ready.resources = root.clone();
    ready.process.executable = executable;
    assert!(verify_component(&manifest, &ready, &root).is_ok());
    assert!(matches!(
        NativeHostServer::<NativeUpdateRequest, NativeUpdateResult>::bind(
            ready.clone(),
            manifest.clone(),
            HashSet::new()
        ),
        Err(NativeHostBindError::InventoryRejected(_))
    ));
    manifest.server_executable = "../MacOS/domios-server".into();
    assert!(verify_component(&manifest, &ready, &root).is_err());
    manifest.server_executable = relative.into();
    manifest.server_sha256 = "f".repeat(64);
    assert!(verify_component(&manifest, &ready, &root).is_err());
}

#[test]
fn component_manifest_pins_server_protocol_build_path_and_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server-fixture.bin");
    let bytes = b"server fixture image";
    let (manifest, ready) = server_fixture(path, bytes);
    let valid_manifest = manifest.clone();

    assert!(verify_component(&manifest, &ready, &ready.resources).is_ok());

    let mut wrong_digest = manifest.clone();
    wrong_digest.server_sha256 = "b".repeat(64);
    assert!(verify_component(&wrong_digest, &ready, &ready.resources).is_err());

    let mut wrong_build = manifest.clone();
    wrong_build.server_build = "other-build".into();
    assert!(verify_component(&wrong_build, &ready, &ready.resources).is_err());

    let mut absolute_path = manifest.clone();
    absolute_path.server_executable = ready.process.executable.clone();
    assert!(verify_component(&absolute_path, &ready, &ready.resources).is_err());

    let mut escaping_path = manifest;
    escaping_path.server_executable = PathBuf::from("../server-fixture.bin");
    assert!(verify_component(&escaping_path, &ready, &ready.resources).is_err());
    assert!(verify_component(&valid_manifest, &ready, ready.resources.parent().unwrap()).is_err());
}

#[test]
fn native_host_bind_separates_inventory_rejection_from_listener_unavailability() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server-fixture.bin");
    let (manifest, ready) = server_fixture(path, b"server fixture image");

    let mut invalid_manifest = manifest.clone();
    invalid_manifest.schema = 0;
    let inventory_error =
        NativeHostServer::<String, String>::bind(ready.clone(), invalid_manifest, HashSet::new());
    assert!(matches!(
        inventory_error,
        Err(NativeHostBindError::InventoryRejected(_))
    ));

    let mut unavailable_ready = ready;
    unavailable_ready.bootstrap_address = "invalid-bootstrap-address".into();
    let listener_error =
        NativeHostServer::<String, String>::bind(unavailable_ready, manifest, HashSet::new());
    assert!(matches!(
        listener_error,
        Err(NativeHostBindError::ListenerUnavailable(_))
    ));
}

fn update_request_and_connection() -> (NativeUpdateRequest, NativeHostConnection) {
    let manifest_executable = PathBuf::from("domios-server");
    let process_executable = std::env::current_dir()
        .unwrap()
        .join("resources")
        .join(&manifest_executable);
    let components = ServerComponentManifest {
        schema: SERVER_COMPONENT_SCHEMA,
        server_build: "server-build-a".into(),
        server_executable: manifest_executable,
        server_sha256: "a".repeat(64),
        client_identity: fixture_client_identity("b".repeat(64)),
        api_version: API_VERSION,
        bootstrap_protocol: BOOTSTRAP_PROTOCOL,
    };
    let identity = ServerIdentity {
        data: PathBuf::from("/tmp/domios-data"),
        config: PathBuf::from("/tmp/domios-config"),
    };
    let request = NativeUpdateRequest {
        operation_id: NativeUpdateOperationId::new(uuid::Uuid::from_u128(101)).unwrap(),
        action: NativeUpdateAction::ClientOnly,
        expected_server: ExpectedServer {
            components: components.clone(),
            identity: identity.clone(),
            incarnation: codeterm_types::server_bootstrap::ServerExpectation {
                process: ProcessProvenance {
                    pid: 41,
                    created: "server-created".into(),
                    executable: process_executable,
                },
                epoch: "server-epoch".into(),
            },
        },
        staged_components: ServerComponentManifest {
            client_identity: fixture_client_identity("c".repeat(64)),
            ..components
        },
        restart_authorized: false,
    };
    let connection = NativeHostConnection {
        instance_id: "instance-a".into(),
        identity,
        server_epoch: "server-epoch".into(),
        generation: 7,
    };
    (request, connection)
}

#[test]
fn native_update_dispatch_maps_only_exact_native_results() {
    let (request, connection) = update_request_and_connection();
    let reply = NativeUpdateResult {
        operation_id: request.operation_id,
        sequence: 1,
        connection: connection.clone(),
        state: NativeUpdateState::InProgress {
            stage: NativeUpdateStage::Applying,
        },
    };
    assert_eq!(
        super::enrollment::map_dispatch_completion(
            &connection,
            &request,
            NativeHostCompletion::Completed(NativeHostResult::NativeUpdate(reply.clone()))
        ),
        codeterm_types::native_update::NativeUpdateDispatchOutcome::Reply(reply)
    );
    assert_eq!(
        super::enrollment::map_dispatch_completion(
            &connection,
            &request,
            NativeHostCompletion::Rejected(NativeHostRejection::Unsupported)
        ),
        codeterm_types::native_update::NativeUpdateDispatchOutcome::RefusedBeforeExecution {
            reason: NativeUpdateRefusal::NativeHostUnavailable,
        }
    );
    assert_eq!(
        super::enrollment::map_dispatch_completion(
            &connection,
            &request,
            NativeHostCompletion::Uncertain(NativeHostUncertainty::DeadlineAfterDispatch)
        ),
        codeterm_types::native_update::NativeUpdateDispatchOutcome::UncertainAfterExecution {
            reason: NativeUpdateUncertainty::DeadlineAfterDispatch,
        }
    );
}

#[test]
fn native_update_observation_preserves_exact_evidence_and_failures() {
    let (request, connection) = update_request_and_connection();
    let no_evidence = super::enrollment::map_observation_completion(
        &connection,
        &request,
        NativeHostCompletion::Completed(NativeHostResult::NativeUpdateObservation(
            NativeUpdateObservationOutcome::NoNewEvidence,
        )),
        None,
        0,
    );
    assert_eq!(no_evidence, NativeUpdateObservationOutcome::NoNewEvidence);

    let unavailable = NativeUpdateObservationOutcome::Unavailable {
        reason: NativeUpdateUncertainty::DisconnectedAfterDispatch,
    };
    assert_eq!(
        super::enrollment::map_observation_completion(
            &connection,
            &request,
            NativeHostCompletion::Completed(NativeHostResult::NativeUpdateObservation(
                unavailable.clone(),
            )),
            None,
            0,
        ),
        unavailable
    );

    let reply = NativeUpdateResult {
        operation_id: request.operation_id,
        sequence: 3,
        connection: connection.clone(),
        state: NativeUpdateState::InProgress {
            stage: NativeUpdateStage::Applying,
        },
    };
    assert_eq!(
        super::enrollment::map_observation_completion(
            &connection,
            &request,
            NativeHostCompletion::Completed(NativeHostResult::NativeUpdateObservation(
                NativeUpdateObservationOutcome::Reply(reply.clone()),
            )),
            None,
            2,
        ),
        NativeUpdateObservationOutcome::Reply(reply.clone())
    );

    let stale = NativeUpdateResult {
        sequence: 2,
        ..reply.clone()
    };
    assert_eq!(
        super::enrollment::map_observation_completion(
            &connection,
            &request,
            NativeHostCompletion::Completed(NativeHostResult::NativeUpdateObservation(
                NativeUpdateObservationOutcome::Reply(stale),
            )),
            None,
            2,
        ),
        NativeUpdateObservationOutcome::Unavailable {
            reason: NativeUpdateUncertainty::InvalidReplyAfterDispatch,
        }
    );
}

#[test]
fn native_host_frame_round_trips_typed_observation_variants() {
    let (request, connection) = update_request_and_connection();
    let result = NativeUpdateResult {
        operation_id: request.operation_id,
        sequence: 1,
        connection: connection.clone(),
        state: NativeUpdateState::InProgress {
            stage: NativeUpdateStage::Applying,
        },
    };
    for observation in [
        NativeUpdateObservationOutcome::Reply(result),
        NativeUpdateObservationOutcome::NoNewEvidence,
        NativeUpdateObservationOutcome::Unavailable {
            reason: NativeUpdateUncertainty::InvalidReplyAfterDispatch,
        },
    ] {
        let frame =
            NativeHostFrame::<NativeUpdateRequest, NativeUpdateResult>::Reply(NativeHostReply {
                request_id: NativeHostRequestId(8),
                connection: connection.clone(),
                scope: NativeHostScope::Client,
                completion: NativeHostCompletion::Completed(
                    NativeHostResult::NativeUpdateObservation(observation),
                ),
            });
        let bytes = serde_json::to_vec(&frame).unwrap();
        let decoded: NativeHostFrame<NativeUpdateRequest, NativeUpdateResult> =
            serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, frame);
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
#[ignore = "local only: intermittent provenance change on GitHub-hosted Linux; run with --run-ignored only"]
fn client_process_must_match_manifest_digest() {
    let client = client_process::Client::new();
    let process = PinnedProcess::open(client.id()).unwrap();
    let image = process.provenance().unwrap().executable;
    let allowed = HashSet::new();
    let mut trusted = policy(
        connection(1),
        digest(&fs::read(image).unwrap()),
        allowed.clone(),
    );
    if let Err(error) = super::enrollment::verify_client_process(&process, &trusted) {
        panic!("the matching client process must verify: {error}");
    }

    trusted.client_identity = fixture_client_identity("c".repeat(64));
    assert!(super::enrollment::verify_client_process(&process, &trusted).is_err());
}

#[test]
fn enrollment_binds_server_generation_challenge_and_allowed_origin() {
    let origin = "http://127.0.0.1:7686".to_string();
    let policy = policy(
        connection(7),
        digest(b"enrollment fixture client"),
        HashSet::from([origin.clone()]),
    );
    let view = NativeViewBinding::observed(origin).unwrap();
    let exact = request(
        policy.challenge.connection.clone(),
        policy.challenge.challenge.clone(),
        Some(view),
    );
    assert!(enroll(&policy, exact, verified_client(&policy)).is_ok());

    let stale_generation = request(connection(6), policy.challenge.challenge.clone(), None);
    assert_eq!(
        enroll(&policy, stale_generation, verified_client(&policy)),
        Err(NativeHostRejection::Unauthorized)
    );

    let stale_challenge = request(
        policy.challenge.connection.clone(),
        "stale-challenge".into(),
        None,
    );
    assert_eq!(
        enroll(&policy, stale_challenge, verified_client(&policy)),
        Err(NativeHostRejection::Unauthorized)
    );

    let foreign_view = NativeViewBinding::observed("https://untrusted.example".into()).unwrap();
    let foreign_origin = request(
        policy.challenge.connection.clone(),
        policy.challenge.challenge.clone(),
        Some(foreign_view),
    );
    assert_eq!(
        enroll(&policy, foreign_origin, verified_client(&policy)),
        Err(NativeHostRejection::InvalidView)
    );
}

#[tokio::test]
async fn releasing_view_rejects_queued_calls_before_dispatch() {
    let view = NativeViewBinding::observed("http://127.0.0.1:7686".into()).unwrap();
    let session = session(Some(view.clone()));
    let request_id = NativeHostRequestId(17);
    let (reply, response) = oneshot::channel();
    session.state.lock().pending.insert(
        request_id,
        Pending {
            scope: NativeHostScope::View(view.session_id),
            reply: Some(reply),
            dispatched: false,
        },
    );

    session.release_view(view.session_id);

    assert_eq!(
        response.await.unwrap().completion,
        NativeHostCompletion::Rejected(NativeHostRejection::InvalidView)
    );
    assert!(!session.mark_dispatched(request_id));
    assert!(!session.state.lock().pending.contains_key(&request_id));
}

#[tokio::test]
async fn replaced_connection_generation_cannot_dispatch_queued_work() {
    let session = session(None);
    let request_id = NativeHostRequestId(19);
    let (reply, _response) = oneshot::channel();
    session.state.lock().pending.insert(
        request_id,
        Pending {
            scope: NativeHostScope::Client,
            reply: Some(reply),
            dispatched: false,
        },
    );
    session
        .active_generation
        .store(10, std::sync::atomic::Ordering::Release);

    assert!(!session.mark_dispatched(request_id));
    assert!(!session.state.lock().pending[&request_id].dispatched);
}

#[tokio::test]
async fn released_dispatched_view_call_stays_uncertain_and_fences_late_reply() {
    let view = NativeViewBinding::observed("http://127.0.0.1:7686".into()).unwrap();
    let session = session(Some(view.clone()));
    let request_id = NativeHostRequestId(23);
    let (reply, response) = oneshot::channel();
    let scope = NativeHostScope::View(view.session_id);
    session.state.lock().pending.insert(
        request_id,
        Pending {
            scope: scope.clone(),
            reply: Some(reply),
            dispatched: true,
        },
    );

    session.release_view(view.session_id);

    assert_eq!(
        response.await.unwrap().completion,
        NativeHostCompletion::Uncertain(NativeHostUncertainty::CancelledAfterDispatch)
    );
    assert!(session.state.lock().pending.contains_key(&request_id));

    let mut stale_connection = session.connection.clone();
    stale_connection.generation += 1;
    session.accept_reply(NativeHostReply {
        request_id,
        connection: stale_connection,
        scope: scope.clone(),
        completion: NativeHostCompletion::Completed(NativeHostResult::Acknowledged),
    });
    assert!(session.state.lock().pending.contains_key(&request_id));

    session.accept_reply(NativeHostReply {
        request_id,
        connection: session.connection.clone(),
        scope,
        completion: NativeHostCompletion::Completed(NativeHostResult::Acknowledged),
    });
    assert!(!session.state.lock().pending.contains_key(&request_id));
}

#[test]
fn health_cdp_port_follows_authenticated_view_diagnostics_and_attachment_lifetime() {
    use std::sync::atomic::Ordering;

    use codeterm_types::native_host::{
        NativeWebviewDiagnosticsReport, NativeWebviewDiagnosticsStatus,
    };
    let directory = tempfile::tempdir().unwrap();
    let (manifest, ready) = server_fixture(
        directory.path().join("server-fixture.bin"),
        b"server fixture image",
    );
    let endpoint = NativeHostEndpoint::<String, String> {
        active: Arc::new(RwLock::new(None)),
        active_generation: Arc::new(AtomicU64::new(9)),
        attached_once: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        next_request_id: Arc::new(AtomicU64::new(1)),
        next_event_id: Arc::new(AtomicU64::new(1)),
        ready,
        manifest,
    };
    assert_eq!(endpoint.cdp_port(), None);
    assert_eq!(endpoint.frontend_port(), None);
    let view = NativeViewBinding::observed("http://127.0.0.1:7686".into()).unwrap();
    let client = session(Some(view.clone()));
    *endpoint.active.write() = Some(client.clone());
    assert_eq!(endpoint.frontend_port(), Some(7686));
    endpoint.active_generation.store(10, Ordering::Release);
    assert_eq!(endpoint.frontend_port(), None);
    endpoint.active_generation.store(9, Ordering::Release);
    assert_eq!(endpoint.cdp_port(), None);
    let report = NativeWebviewDiagnosticsReport {
        connection: client.connection.clone(),
        view_session: view.session_id,
        status: NativeWebviewDiagnosticsStatus::Active {
            port: 9398,
            view_generation: 1,
        },
    };
    let mut stale = report.clone();
    stale.connection.generation += 1;
    client.accept_diagnostics_status(stale);
    assert_eq!(endpoint.cdp_port(), None);
    client.accept_diagnostics_status(report.clone());
    assert_eq!(endpoint.cdp_port(), Some(9398));
    client.release_view(view.session_id);
    assert_eq!(endpoint.frontend_port(), None);
    assert_eq!(endpoint.cdp_port(), None);
    client.bind_view(view);
    client.accept_diagnostics_status(report);
    assert_eq!(endpoint.cdp_port(), Some(9398));
    client.retire(NativeHostUncertainty::DisconnectedAfterDispatch);
    assert_eq!(endpoint.frontend_port(), None);
    assert_eq!(endpoint.cdp_port(), None);
    endpoint.active.write().take();
    assert_eq!(endpoint.cdp_port(), None);
}
