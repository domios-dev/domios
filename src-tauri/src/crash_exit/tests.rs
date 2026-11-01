use super::*;
use codeterm_types::server_bootstrap::{ProcessProvenance, ServerExpectation, ServerIdentity};
use std::{net::SocketAddr, path::PathBuf};

#[test]
fn recovery_cancellation_follows_the_exact_exit_gate() {
    let gate = ClientExitGate::default();
    let mut exit = gate.events.subscribe();
    assert!(!*exit.borrow_and_update());
    let first = gate.begin(ClientExitReason::ExplicitQuit).unwrap();
    assert!(*exit.borrow_and_update());
    assert!(gate.refuse_before_dispatch(first));
    assert!(!*exit.borrow_and_update());
    let second = gate.begin(ClientExitReason::ExplicitQuit).unwrap();
    assert!(*exit.borrow_and_update());
    assert!(!gate.refuse_before_dispatch(first));
    assert!(*exit.borrow_and_update());
    assert!(gate.mark_uncertain(second));
    assert!(gate.confirm_client_only());
    assert!(*exit.borrow_and_update());
}

fn ready(epoch: &str, pid: u32) -> ServerReady {
    ServerReady {
        protocol: BOOTSTRAP_PROTOCOL,
        api_version: 3,
        build: "server-build".to_string(),
        instance_id: "split-oracle-quit".to_string(),
        identity: ServerIdentity {
            data: PathBuf::from("/probe/data"),
            config: PathBuf::from("/probe/config"),
        },
        epoch: epoch.to_string(),
        process: ProcessProvenance {
            pid,
            created: "process-created".to_string(),
            executable: PathBuf::from("/probe/domios-server"),
        },
        api_address: "127.0.0.1:7860".parse::<SocketAddr>().unwrap(),
        socket_address: "probe.sock".to_string(),
        bootstrap_address: "bootstrap.sock".to_string(),
        resources: PathBuf::from("/probe/resources"),
    }
}

fn reply(expected: &ServerReady, challenge: &str) -> AttachReply {
    AttachReply {
        challenge: challenge.to_string(),
        ready: expected.clone(),
        api_token: None,
    }
}

fn shutdown_request(expected: &ServerReady, challenge: &str) -> AttachRequest {
    AttachRequest {
        protocol: expected.protocol,
        api_version: expected.api_version,
        build: expected.build.clone(),
        identity: expected.identity.clone(),
        challenge: challenge.to_string(),
        operation: BootstrapOperation::Shutdown,
        expected: Some(ServerExpectation {
            process: expected.process.clone(),
            epoch: expected.epoch.clone(),
        }),
    }
}

#[test]
fn lifecycle_actions_keep_window_close_and_crash_separate_from_server_shutdown() {
    assert_eq!(
        plan_client_lifecycle(ClientLifecycleEvent::ServerUnavailable),
        ClientLifecycleAction::ShowReconnectingAndPreserveUi
    );
    assert_eq!(
        plan_client_lifecycle(ClientLifecycleEvent::MainWindowClose),
        ClientLifecycleAction::CloseWindowKeepOwners
    );
    assert_eq!(
        plan_client_lifecycle(ClientLifecycleEvent::ClientCrashRelaunch),
        ClientLifecycleAction::ReconnectToExistingServer
    );
    assert_eq!(
        plan_client_lifecycle(ClientLifecycleEvent::ExplicitQuit),
        ClientLifecycleAction::ShutdownExactServerThenExitClient
    );
    assert_eq!(
        plan_client_lifecycle(ClientLifecycleEvent::AuthorizedServerOrPairUpdate),
        ClientLifecycleAction::ShutdownExactServerThenExitClient
    );
    assert_eq!(
        plan_client_lifecycle(ClientLifecycleEvent::ClientOnlyUpdate),
        ClientLifecycleAction::ExitClientOnly
    );
    assert_eq!(
        plan_client_lifecycle(ClientLifecycleEvent::UnixSignal),
        ClientLifecycleAction::ExitClientOnly
    );
}

#[test]
fn exit_gate_coalesces_requests_and_only_releases_proven_predispatch_refusal() {
    let gate = ClientExitGate::default();
    let ticket = gate.begin(ClientExitReason::ExplicitQuit).unwrap();
    assert_eq!(
        gate.begin(ClientExitReason::ExplicitQuit),
        Err(ClientExitRefusal::AlreadyInProgress)
    );
    assert!(gate.refuse_before_dispatch(ticket));

    let next = gate.begin(ClientExitReason::ExplicitQuit).unwrap();
    assert!(!gate.refuse_before_dispatch(ticket));
    assert!(gate.mark_uncertain(next));
    assert!(!gate.refuse_before_dispatch(next));
    assert!(gate.status().is_some_and(|status| {
        status.operation_id == next.operation_id && status.phase == ClientExitPhase::Uncertain
    }));
    assert!(gate.confirm(next));
    assert!(!gate.mark_uncertain(next));
    assert!(gate.status().is_some_and(|status| {
        status.operation_id == next.operation_id && status.phase == ClientExitPhase::Confirmed
    }));
}

#[test]
fn client_only_exit_never_preempts_an_in_flight_shutdown_and_resolves_uncertainty() {
    let idle = ClientExitGate::default();
    assert!(idle.confirm_client_only());
    assert!(idle.status().is_some_and(|status| {
        status.reason == ClientExitReason::ClientOnlyQuit
            && status.phase == ClientExitPhase::Confirmed
    }));

    let gate = ClientExitGate::default();
    let ticket = gate.begin(ClientExitReason::ExplicitQuit).unwrap();
    assert!(!gate.confirm_client_only());
    assert!(gate.mark_uncertain(ticket));
    assert!(gate.confirm_client_only());
    assert!(gate.status().is_some_and(|status| {
        status.operation_id == ticket.operation_id
            && status.reason == ClientExitReason::ExplicitQuit
            && status.phase == ClientExitPhase::Confirmed
    }));
    assert!(gate.confirm_client_only());
}

#[test]
fn shutdown_reply_requires_the_exact_server_snapshot_and_challenge() {
    let expected = ready("epoch-old", 41);
    assert!(is_exact_server_shutdown_reply(
        &expected,
        &shutdown_request(&expected, "challenge-1"),
        &reply(&expected, "challenge-1")
    ));
    assert!(!is_exact_server_shutdown_reply(
        &expected,
        &shutdown_request(&expected, "challenge-1"),
        &reply(&ready("epoch-new", 41), "challenge-1")
    ));
    assert!(!is_exact_server_shutdown_reply(
        &expected,
        &shutdown_request(&expected, "challenge-1"),
        &reply(&ready("epoch-old", 42), "challenge-1")
    ));
    let mut wrong_operation = shutdown_request(&expected, "challenge-1");
    wrong_operation.operation = BootstrapOperation::Inspect;
    assert!(!is_exact_server_shutdown_reply(
        &expected,
        &wrong_operation,
        &reply(&expected, "challenge-1")
    ));
    let mut wrong_expected = shutdown_request(&expected, "challenge-1");
    wrong_expected.expected = Some(ServerExpectation {
        process: expected.process.clone(),
        epoch: "epoch-new".to_string(),
    });
    assert!(!is_exact_server_shutdown_reply(
        &expected,
        &wrong_expected,
        &reply(&expected, "challenge-1")
    ));
    assert!(!is_exact_server_shutdown_reply(
        &expected,
        &shutdown_request(&expected, "challenge-1"),
        &reply(&expected, "challenge-stale")
    ));
}

#[test]
fn shutdown_reply_requires_the_current_shared_bootstrap_protocol() {
    let expected = ready("epoch-old", 41);
    let previous_protocol = BOOTSTRAP_PROTOCOL.saturating_sub(1);
    let mut previous_ready = expected.clone();
    previous_ready.protocol = previous_protocol;
    let previous_request = shutdown_request(&previous_ready, "challenge-1");
    assert!(!is_exact_server_shutdown_reply(
        &previous_ready,
        &previous_request,
        &reply(&previous_ready, "challenge-1")
    ));

    let mut previous_request = shutdown_request(&expected, "challenge-1");
    previous_request.protocol = previous_protocol;
    assert!(!is_exact_server_shutdown_reply(
        &expected,
        &previous_request,
        &reply(&expected, "challenge-1")
    ));
}
