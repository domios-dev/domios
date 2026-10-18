use super::*;
fn snapshot() -> QuitSnapshot {
    use codeterm_types::server_bootstrap::{
        ProcessProvenance, ServerIdentity, ServerReady, API_VERSION, BOOTSTRAP_PROTOCOL,
    };
    let root = std::path::PathBuf::from(if cfg!(windows) {
        "C:/quit-fixture"
    } else {
        "/quit-fixture"
    });
    QuitSnapshot {
        ready: ServerReady {
            protocol: BOOTSTRAP_PROTOCOL,
            api_version: API_VERSION,
            build: "expected-server-build".into(),
            instance_id: "instance-a".into(),
            identity: ServerIdentity {
                data: root.join("data"),
                config: root.join("config.toml"),
            },
            epoch: "epoch-a".into(),
            process: ProcessProvenance {
                pid: 42,
                created: "start-a".into(),
                executable: root.join("server"),
            },
            api_address: "127.0.0.1:18886".parse().unwrap(),
            socket_address: "socket".into(),
            bootstrap_address: "bootstrap".into(),
            resources: root.join("resources"),
        },
        client: None,
    }
}

#[tokio::test]
async fn exact_quit_cleanup_success_is_required_and_admission_never_replays() {
    let (quit, mut requests) = ExactQuitCoordinator::new();
    quit.install(snapshot().ready, None);
    let expected = quit.snapshot().unwrap();
    let task = {
        let quit = quit.clone();
        let expected = expected.clone();
        tokio::spawn(async move { quit.request(expected).await })
    };
    let admitted = requests.recv().await.unwrap();
    assert!(admitted.matches(&expected));
    assert!(quit.admit_headless(&admitted));
    quit.complete(&Ok(()));
    assert!(
        matches!(task.await.unwrap(), QuitApiReply::Completed { expected: actual, .. } if actual.matches(&expected))
    );
    assert!(quit.completion_sent());
    assert!(matches!(
        quit.request(expected).await,
        QuitApiReply::Refused {
            reason: QuitRefusal::Busy
        }
    ));
    assert!(requests.try_recv().is_err());
}

#[tokio::test]
async fn failed_cleanup_stays_uncertain_and_holds_admission() {
    let (quit, mut requests) = ExactQuitCoordinator::new();
    quit.install(snapshot().ready, None);
    let task = {
        let quit = quit.clone();
        tokio::spawn(async move { quit.request(snapshot()).await })
    };
    requests.recv().await.unwrap();
    quit.complete(&Err(io::Error::other("injected persistence failure")));
    assert!(matches!(
        task.await.unwrap(),
        QuitApiReply::Uncertain {
            reason: QuitUncertainty::InvalidCompletion
        }
    ));
    assert!(!quit.completion_sent());
    assert!(matches!(
        quit.request(snapshot()).await,
        QuitApiReply::Refused {
            reason: QuitRefusal::Busy
        }
    ));
    assert!(requests.try_recv().is_err());
}

#[tokio::test]
async fn wrong_incarnation_or_build_refuses_before_shutdown_event() {
    let (quit, mut requests) = ExactQuitCoordinator::new();
    quit.install(snapshot().ready, None);
    let mut expected = snapshot();
    expected.ready.build = "other-build".into();
    assert!(matches!(
        quit.request(expected).await,
        QuitApiReply::Refused {
            reason: QuitRefusal::MismatchedBuild
        }
    ));
    let mut expected = snapshot();
    expected.ready.process.created = "reused-pid".into();
    assert!(matches!(
        quit.request(expected).await,
        QuitApiReply::Refused {
            reason: QuitRefusal::IdentityMismatch
        }
    ));
    assert!(requests.try_recv().is_err());
}

#[tokio::test]
async fn headless_race_rechecks_owner_before_cleanup() {
    let (quit, mut requests) = ExactQuitCoordinator::new();
    quit.install(snapshot().ready, None);
    let task = {
        let quit = quit.clone();
        tokio::spawn(async move { quit.request(snapshot()).await })
    };
    let expected = requests.recv().await.unwrap();
    let mut next = snapshot().ready;
    next.epoch = "replacement".into();
    quit.install(next, None);
    assert!(!quit.admit_headless(&expected));
    assert!(matches!(
        task.await.unwrap(),
        QuitApiReply::Refused {
            reason: QuitRefusal::IdentityMismatch
        }
    ));
    assert!(!quit.completion_sent());
}
