use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use serde_json::json;

fn emulator() -> Endpoints {
    Endpoints {
        identity_toolkit: "http://127.0.0.1:19099/identitytoolkit.googleapis.com/v1/",
        secure_token: "http://127.0.0.1:19099/securetoken.googleapis.com/v1/",
        functions: "http://127.0.0.1:15001/demo-domios/us-central1/",
        signin: "http://127.0.0.1:15000/signin/",
        redeem: "http://127.0.0.1:15001/demo-domios/us-central1/redeemDesktopLoginCode",
        emulator: true,
        qa: false,
    }
}

fn qa() -> Endpoints {
    Endpoints {
        functions: "https://us-central1-domios-signin-qa.cloudfunctions.net/",
        signin: "https://domios-signin-qa.web.app/signin/",
        redeem: "https://us-central1-domios-signin-qa.cloudfunctions.net/redeemDesktopLoginCode",
        qa: true,
        ..PRODUCTION
    }
}

#[test]
fn trusted_selection_matrix_preserves_cloud_opt_out_release_and_qa_refusals() {
    for debug in [false, true] {
        let production = Selection::resolve(debug, PRODUCTION).unwrap();
        assert_eq!(
            production.firestore,
            FirestoreTarget::google("domios-d9357").unwrap()
        );
        assert_eq!(production.endpoints.functions, PRODUCTION.functions);
    }
    let qa = Selection::resolve(true, qa()).unwrap();
    assert_eq!(
        qa.firestore,
        FirestoreTarget::google("domios-signin-qa").unwrap()
    );
    assert_eq!(
        qa.endpoints.functions,
        "https://us-central1-domios-signin-qa.cloudfunctions.net/"
    );
    assert_eq!(Selection::resolve(false, emulator()), Err(Error::Transport));
    assert_eq!(
        Selection::resolve(false, qa.endpoints),
        Err(Error::Transport)
    );
    for mixed in [
        Endpoints {
            qa: true,
            ..emulator()
        },
        Endpoints {
            emulator: true,
            ..qa.endpoints
        },
        Endpoints {
            emulator: false,
            ..emulator()
        },
        Endpoints {
            functions: PRODUCTION.functions,
            ..emulator()
        },
        Endpoints {
            secure_token: PRODUCTION.secure_token,
            ..emulator()
        },
        Endpoints {
            signin: PRODUCTION.signin,
            ..emulator()
        },
        Endpoints {
            functions: emulator().functions,
            ..PRODUCTION
        },
        Endpoints {
            functions: "http://localhost:15001/demo-domios/us-central1/",
            ..emulator()
        },
        Endpoints {
            functions: "http://127.0.0.1:15001/other/us-central1/",
            ..emulator()
        },
        Endpoints {
            functions: "http://user:secret@127.0.0.1:15001/demo-domios/us-central1/",
            ..emulator()
        },
    ] {
        assert_eq!(Selection::resolve(true, mixed), Err(Error::Transport));
    }
}

fn bound(selection: Selection, current: fn() -> Endpoints) -> SelectedCalls {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    SelectedCalls {
        calls: Calls {
            service: fixture.service.clone(),
            uid: "own-account".into(),
            generation: 7,
        },
        selection,
        current,
    }
}

#[test]
fn registry_and_both_stream_constructors_share_the_captured_selection() {
    let mut selections = vec![
        (
            Selection::resolve(true, PRODUCTION).unwrap(),
            (|| PRODUCTION) as fn() -> Endpoints,
        ),
        (
            Selection::resolve(true, qa()).unwrap(),
            qa as fn() -> Endpoints,
        ),
    ];
    if cfg!(debug_assertions) {
        selections.push((
            Selection::resolve(true, emulator()).unwrap(),
            emulator as fn() -> Endpoints,
        ));
    }
    for (selection, current) in selections {
        let calls: Arc<dyn AuthenticatedCall> = Arc::new(bound(selection.clone(), current));
        let registry = codeterm_neighbors::adapter::FirebaseRegistry {
            calls: calls.clone(),
            uid: "own-account".into(),
            installation_id: "own-installation".into(),
            machine_label: "Fixture".into(),
        };
        let runtime = selection
            .transport(
                registry.calls.clone(),
                registry.uid.clone(),
                registry.installation_id.clone(),
            )
            .unwrap();
        let updates = selection
            .transport(calls, "own-account".into(), "own-installation".into())
            .unwrap();
        for transport in [&runtime, &updates] {
            assert_eq!(transport.project, selection.firestore.project());
            assert_eq!(transport.uid, "own-account");
            assert_eq!(transport.installation_id, "own-installation");
            assert_eq!(
                transport
                    .calls
                    .firestore_target(&transport.project)
                    .unwrap(),
                selection.firestore
            );
            assert!(Arc::ptr_eq(&transport.calls, &registry.calls));
        }
        assert_eq!(
            registry.calls.firestore_target("foreign-project"),
            Err(Error::Transport)
        );
    }
}

#[tokio::test]
async fn changed_target_refuses_before_account_or_callable_use() {
    let calls = bound(Selection::resolve(true, PRODUCTION).unwrap(), emulator);
    assert_eq!(calls.firestore_target("domios-d9357"), Err(Error::Revoked));
    assert!(matches!(calls.id_token().await, Err(Error::Revoked)));
    assert_eq!(
        calls.invoke("friendsEnrollVerifierKey", json!({})).await,
        Err(Error::Revoked)
    );
    assert_eq!(
        calls.invoke("friendsSetFriendReceiver", json!({})).await,
        Err(Error::Revoked)
    );
}

#[tokio::test]
async fn selected_transport_keeps_the_existing_account_guard() {
    let calls = bound(Selection::resolve(true, PRODUCTION).unwrap(), || PRODUCTION);
    assert_eq!(
        calls.invoke("friendsSetFriendReceiver", json!({})).await,
        Err(Error::Revoked)
    );
}

#[test]
fn mixed_constructor_target_refuses_without_account_access() {
    let production = Selection::resolve(true, PRODUCTION).unwrap();
    let calls = Arc::new(bound(Selection::resolve(true, qa()).unwrap(), qa));
    assert!(matches!(
        production.transport(calls, "own-account".into(), "own-installation".into()),
        Err(Error::Transport)
    ));
}
