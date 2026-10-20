use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
struct Port(AtomicUsize);
impl AppQuitPort for Port {
    fn snapshot(&self) -> Result<QuitSnapshot, QuitRefusal> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(QuitRefusal::NoInstance)
    }
    fn request(&self, _: QuitSnapshot) -> QuitReplyFuture<'_> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            QuitApiReply::Refused {
                reason: QuitRefusal::Busy,
            }
        })
    }
}
#[tokio::test]
async fn exact_quit_refuses_before_dispatch_without_local_admission_or_valid_body() {
    let port = Arc::new(Port(AtomicUsize::new(0)));
    let handle = AppQuitPortHandle(port.clone());
    assert!(matches!(
        dispatch(None, Some(&handle), Err(())).await,
        QuitApiReply::Refused {
            reason: QuitRefusal::IdentityMismatch
        }
    ));
    assert!(matches!(
        dispatch(Some(&CallerIdentity::Local), Some(&handle), Err(())).await,
        QuitApiReply::Refused {
            reason: QuitRefusal::IdentityMismatch
        }
    ));
    assert!(matches!(
        dispatch(
            Some(&CallerIdentity::ServicePush {
                service_id: "foreign-service".into()
            }),
            Some(&handle),
            Err(())
        )
        .await,
        QuitApiReply::Refused {
            reason: QuitRefusal::IdentityMismatch
        }
    ));
    assert_eq!(port.0.load(Ordering::SeqCst), 0);
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

#[tokio::test]
async fn admitted_local_request_reaches_the_single_typed_port_once() {
    let port = Arc::new(Port(AtomicUsize::new(0)));
    let handle = AppQuitPortHandle(port.clone());
    assert!(matches!(
        dispatch(Some(&CallerIdentity::Local), Some(&handle), Ok(snapshot())).await,
        QuitApiReply::Refused {
            reason: QuitRefusal::Busy
        }
    ));
    assert_eq!(port.0.load(Ordering::SeqCst), 1);
}
