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

#[test]
fn serialized_completion_preserves_native_client_identity_and_rejects_replacement() {
    use codeterm_types::{app_quit::QuitClient, native_host::NativeHostConnection};
    let mut expected = snapshot();
    expected.client = Some(QuitClient {
        connection: NativeHostConnection {
            instance_id: expected.ready.instance_id.clone(),
            identity: expected.ready.identity.clone(),
            server_epoch: expected.ready.epoch.clone(),
            generation: 7,
        },
        process: codeterm_types::server_bootstrap::ProcessProvenance {
            pid: 43,
            created: "native-created".into(),
            executable: expected.ready.resources.join("native-client"),
        },
    });
    let bytes = serde_json::to_vec(&QuitApiReply::Completed {
        expected: Box::new(expected.clone()),
        shutdown_completion: QuitCompletion::default(),
    })
    .unwrap();
    let reply = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        validate_completion(&expected, reply).unwrap(),
        QuitCompletion::default()
    );
    for field in ["generation", "created", "pid"] {
        let mut replacement = expected.clone();
        let client = replacement.client.as_mut().unwrap();
        match field {
            "generation" => client.connection.generation += 1,
            "created" => client.process.created = "replacement".into(),
            _ => client.process.pid += 1,
        }
        assert!(matches!(
            validate_completion(&replacement, serde_json::from_slice(&bytes).unwrap()),
            Err(QuitOutput::Uncertain {
                reason: QuitUncertainty::InvalidCompletion
            })
        ));
    }
}
