use super::enrollment::{
    enroll, verify_client_process, verify_component, EnrollmentPolicy, VerifiedClientProcess,
};
use crate::bootstrap::process::PinnedProcess;
use codeterm_types::{
    native_host::{
        NativeHostChallenge, NativeHostCompletion, NativeHostConnection, NativeHostEnrollmentRequest,
        NativeHostRejection, NativeHostReply, NativeHostRequestId, NativeHostResult,
        NativeHostScope, NativeHostUncertainty, NativeViewBinding, NATIVE_HOST_PROTOCOL,
    },
    server_bootstrap::{
        ProcessProvenance, ServerComponentManifest, ServerIdentity, ServerReady,
        API_VERSION, BOOTSTRAP_PROTOCOL, SERVER_COMPONENT_SCHEMA,
    },
};
use super::transport::{Pending, Session, SessionState};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{
        atomic::AtomicBool,
        Arc,
    },
};
use tokio::sync::{mpsc, oneshot, watch};

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

fn server_fixture(path: PathBuf, bytes: &[u8]) -> (ServerComponentManifest, ServerReady) {
    fs::write(&path, bytes).unwrap();
    let executable = path.canonicalize().unwrap();
    let identity = connection(1).identity;
    let process = ProcessProvenance {
        pid: 42,
        created: "process-created".into(),
        executable: executable.clone(),
    };
    let manifest = ServerComponentManifest {
        schema: SERVER_COMPONENT_SCHEMA,
        server_build: "server-build-a".into(),
        server_executable: executable.clone(),
        server_sha256: digest(bytes),
        client_sha256: "a".repeat(64),
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
        resources: PathBuf::from("resources"),
    };
    (manifest, ready)
}

fn client_sha256() -> String {
    digest(&fs::read(std::env::current_exe().unwrap()).unwrap())
}

fn verified_client(policy: &EnrollmentPolicy) -> VerifiedClientProcess {
    let process = PinnedProcess::open(std::process::id()).unwrap();
    verify_client_process(&process, policy).unwrap()
}

fn policy(
    connection: NativeHostConnection,
    client_sha256: String,
    allowed_view_origins: HashSet<String>,
) -> EnrollmentPolicy {
    EnrollmentPolicy {
        challenge: NativeHostChallenge::new(connection),
        client_sha256,
        allowed_view_origins,
    }
}

fn request(
    policy: &EnrollmentPolicy,
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
        allowed_view_origins: HashSet::from(["http://127.0.0.1:7686".into()]),
        control,
        events,
        state: Mutex::new(SessionState {
            view,
            pending: HashMap::new(),
        }),
        shutdown,
        retired: AtomicBool::new(false),
    })
}

#[test]
fn component_manifest_pins_server_protocol_build_path_and_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server-fixture.bin");
    let bytes = b"server fixture image";
    let (manifest, ready) = server_fixture(path, bytes);

    assert!(verify_component(&manifest, &ready).is_ok());

    let mut wrong_digest = manifest.clone();
    wrong_digest.server_sha256 = "b".repeat(64);
    assert!(verify_component(&wrong_digest, &ready).is_err());

    let mut wrong_build = manifest;
    wrong_build.server_build = "other-build".into();
    assert!(verify_component(&wrong_build, &ready).is_err());
}

#[test]
fn client_process_must_match_manifest_digest() {
    let process = PinnedProcess::open(std::process::id()).unwrap();
    let allowed = HashSet::new();
    let mut trusted = policy(connection(1), client_sha256(), allowed.clone());
    assert!(verify_client_process(&process, &trusted).is_ok());

    trusted.client_sha256 = "c".repeat(64);
    assert!(verify_client_process(&process, &trusted).is_err());
}

#[test]
fn enrollment_binds_server_generation_challenge_and_allowed_origin() {
    let origin = "http://127.0.0.1:7686".to_string();
    let policy = policy(
        connection(7),
        client_sha256(),
        HashSet::from([origin.clone()]),
    );
    let view = NativeViewBinding::observed(origin).unwrap();
    let exact = request(
        &policy,
        policy.challenge.connection.clone(),
        policy.challenge.challenge.clone(),
        Some(view),
    );
    assert!(enroll(&policy, exact, verified_client(&policy)).is_ok());

    let stale_generation = request(
        &policy,
        connection(6),
        policy.challenge.challenge.clone(),
        None,
    );
    assert_eq!(
        enroll(&policy, stale_generation, verified_client(&policy)),
        Err(NativeHostRejection::Unauthorized)
    );

    let stale_challenge = request(
        &policy,
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
        &policy,
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
