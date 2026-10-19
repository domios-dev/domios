use super::*;
#[test]
fn refused_or_uncertain_is_never_confirmation() {
    let output = refused(QuitRefusal::Busy);
    assert!(matches!(
        output,
        QuitOutput::Refused {
            reason: QuitRefusal::Busy
        }
    ));
    assert!(matches!(
        uncertain(QuitUncertainty::ExitUnobserved),
        QuitOutput::Uncertain { .. }
    ));
    assert!(serde_json::from_str::<QuitApiReply>(r#"{"outcome":"completed","shutdown_completion":{"operation":"shutdown","cleanup":"completed"}}"#).is_err());
}

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

#[test]
fn exact_completion_matches_build_epoch_and_process_creation_before_exit_can_confirm() {
    let expected = snapshot();
    let completion = QuitCompletion::default();
    assert!(validate_completion(
        &expected,
        QuitApiReply::Completed {
            expected: Box::new(expected.clone()),
            shutdown_completion: completion
        }
    )
    .is_ok());
    for field in ["epoch", "creation", "build"] {
        let mut actual = expected.clone();
        match field {
            "epoch" => actual.ready.epoch = "stale".into(),
            "creation" => actual.ready.process.created = "reused".into(),
            _ => actual.ready.build = "wrong".into(),
        }
        assert!(matches!(
            validate_completion(
                &expected,
                QuitApiReply::Completed {
                    expected: Box::new(actual),
                    shutdown_completion: completion
                }
            ),
            Err(QuitOutput::Uncertain {
                reason: QuitUncertainty::InvalidCompletion
            })
        ));
    }
    assert!(matches!(
        validate_completion(
            &expected,
            QuitApiReply::Refused {
                reason: QuitRefusal::Busy
            }
        ),
        Err(QuitOutput::Refused {
            reason: QuitRefusal::Busy
        })
    ));
}
