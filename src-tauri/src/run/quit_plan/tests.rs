use super::*;
use crate::host_ports::native_client::ExactServerShutdownUncertainty;

use codeterm_types::server_bootstrap::{
    ProcessProvenance, ServerIdentity, ServerReady, BOOTSTRAP_PROTOCOL,
};
use std::path::PathBuf;

const ORIGINS: [QuitOrigin; 2] = [QuitOrigin::Local, QuitOrigin::Remote];

#[test]
fn exact_quit_wires_confirmed_ticket_before_client_exit_and_rejects_stale_ticket() {
    use std::cell::RefCell;
    let result = ExactServerShutdownResult::Confirmed {
        server: ready(),
        client_generation: 7,
    };
    let calls = RefCell::new(Vec::new());
    finish_quit(
        QuitOrigin::Remote,
        &result,
        || {
            calls.borrow_mut().push("confirm");
            true
        },
        |resolution| {
            assert_eq!(resolution, QuitResolution::ExitClient);
            calls.borrow_mut().push("exit");
        },
    );
    assert_eq!(*calls.borrow(), ["confirm", "exit"]);
    finish_quit(
        QuitOrigin::Remote,
        &result,
        || false,
        |_| panic!("A stale ticket cannot exit the client"),
    );
}

#[test]
fn exact_quit_uncertainty_keeps_client_open_without_confirming_exit() {
    let result = ExactServerShutdownResult::UncertainAfterDispatch(
        ExactServerShutdownUncertainty::HelperOutcomeUnknown,
    );
    let resolution = std::cell::Cell::new(None);
    finish_quit(
        QuitOrigin::Remote,
        &result,
        || panic!("An uncertain helper cannot confirm the exit ticket"),
        |next| resolution.set(Some(next)),
    );
    assert_eq!(
        resolution.get(),
        Some(QuitResolution::Feedback(QuitFeedback::Uncertain))
    );
}

fn ready() -> ServerReady {
    ServerReady {
        protocol: BOOTSTRAP_PROTOCOL,
        api_version: 3,
        build: "server-build".to_string(),
        instance_id: "quit-plan".to_string(),
        identity: ServerIdentity {
            data: PathBuf::from("/probe/data"),
            config: PathBuf::from("/probe/config"),
        },
        epoch: "epoch".to_string(),
        process: ProcessProvenance {
            pid: 41,
            created: "process-created".to_string(),
            executable: PathBuf::from("/probe/domios-server"),
        },
        api_address: "127.0.0.1:7860".parse().unwrap(),
        socket_address: "probe.sock".to_string(),
        bootstrap_address: "bootstrap.sock".to_string(),
        resources: PathBuf::from("/probe/resources"),
    }
}

#[test]
fn disconnected_or_not_ready_local_quit_closes_the_client_without_a_server_request() {
    assert_eq!(
        plan_quit_start(QuitOrigin::Local, ExitGate::Idle, false),
        QuitStart::CloseClientOnly
    );
    assert_eq!(
        plan_quit_start(QuitOrigin::Remote, ExitGate::Idle, false),
        QuitStart::Unavailable
    );
}

#[test]
fn reachable_quit_keeps_the_coordinated_server_shutdown() {
    for origin in ORIGINS {
        assert_eq!(
            plan_quit_start(origin, ExitGate::Idle, true),
            QuitStart::RequestServerShutdown
        );
        let confirmed = ExactServerShutdownResult::Confirmed {
            server: ready(),
            client_generation: 1,
        };
        assert_eq!(
            plan_quit_result(origin, classify_shutdown(&confirmed)),
            QuitResolution::ExitClient
        );
    }
}

#[test]
fn unreachable_control_plane_discovered_during_quit_closes_the_client_only() {
    for refusal in [
        ExactServerShutdownRefusal::ApiAttachmentUnavailable,
        ExactServerShutdownRefusal::NativeHostUnavailable,
    ] {
        let outcome = classify_shutdown(&ExactServerShutdownResult::RefusedBeforeDispatch(refusal));
        assert_eq!(outcome, ShutdownOutcome::ServerUnreachable);
        assert_eq!(
            plan_quit_result(QuitOrigin::Local, outcome),
            QuitResolution::CloseClientOnly
        );
        assert_eq!(
            plan_quit_result(QuitOrigin::Remote, outcome),
            QuitResolution::Feedback(QuitFeedback::Refused)
        );
    }
}

#[test]
fn failed_request_to_a_reachable_server_offers_closing_the_window_only() {
    for refusal in [
        ExactServerShutdownRefusal::IncompatibleServerProtocol,
        ExactServerShutdownRefusal::StaleIncarnation,
        ExactServerShutdownRefusal::ProcessInspectionFailed,
        ExactServerShutdownRefusal::HelperArtifactInvalid,
        ExactServerShutdownRefusal::VerificationIoFailure,
        ExactServerShutdownRefusal::VerificationDeadline,
        ExactServerShutdownRefusal::VerificationInterrupted,
        ExactServerShutdownRefusal::HelperDeadlineBeforeDispatch,
        ExactServerShutdownRefusal::HelperUnavailable,
    ] {
        let outcome = classify_shutdown(&ExactServerShutdownResult::RefusedBeforeDispatch(refusal));
        assert_eq!(outcome, ShutdownOutcome::RefusedBeforeDispatch);
        assert_eq!(
            plan_quit_result(QuitOrigin::Local, outcome),
            QuitResolution::Feedback(QuitFeedback::OfferCloseAfterRefusal)
        );
    }
    let uncertain = classify_shutdown(&ExactServerShutdownResult::UncertainAfterDispatch(
        ExactServerShutdownUncertainty::ServerExitUnobserved,
    ));
    assert_eq!(
        plan_quit_result(QuitOrigin::Local, uncertain),
        QuitResolution::Feedback(QuitFeedback::OfferCloseAfterUncertainty)
    );
    assert_eq!(
        plan_quit_result(QuitOrigin::Remote, uncertain),
        QuitResolution::Feedback(QuitFeedback::Uncertain)
    );
}

#[test]
fn uncertain_shutdown_never_traps_a_later_local_quit_and_never_replays_the_request() {
    assert_eq!(
        plan_quit_start(QuitOrigin::Local, ExitGate::Uncertain, true),
        QuitStart::Busy(QuitFeedback::OfferCloseAfterUncertainty)
    );
    assert_eq!(
        plan_quit_start(QuitOrigin::Local, ExitGate::Uncertain, false),
        QuitStart::Busy(QuitFeedback::OfferCloseAfterUncertainty)
    );
    assert_eq!(
        plan_quit_start(QuitOrigin::Remote, ExitGate::Uncertain, true),
        QuitStart::Busy(QuitFeedback::Uncertain)
    );
    for origin in ORIGINS {
        for gate in [ExitGate::Requested, ExitGate::Confirmed] {
            for attached in [true, false] {
                assert_eq!(
                    plan_quit_start(origin, gate, attached),
                    QuitStart::Busy(QuitFeedback::Waiting)
                );
            }
        }
    }
}
