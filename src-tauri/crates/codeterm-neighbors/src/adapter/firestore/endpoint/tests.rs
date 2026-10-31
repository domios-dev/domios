use super::*;
use crate::adapter::{AuthenticatedCall, FirebaseTransport};
use crate::receive::NeighborTransport;
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

#[test]
fn exact_channel_policy_matrix() {
    for project in ["domios-d9357", "domios-signin-qa"] {
        for debug in [false, true] {
            let target = FirestoreTarget::resolve(debug, false, GOOGLE, project).unwrap();
            assert_eq!(target.uri(), "https://firestore.googleapis.com");
            assert!(target.tls());
            assert_eq!(target.database(), format!("projects/{project}/databases/(default)"));
        }
    }
    let local = FirestoreTarget::resolve(true, true, EMULATOR, "demo-domios").unwrap();
    assert_eq!(local.uri(), "http://127.0.0.1:18080");
    assert!(!local.tls());
    assert_eq!(local.database(), "projects/demo-domios/databases/(default)");
    for (debug, selected, uri, project) in [
        (false, true, EMULATOR, "demo-domios"),
        (true, false, EMULATOR, "demo-domios"),
        (true, true, GOOGLE, "demo-domios"),
        (true, false, GOOGLE, "demo-domios"),
        (true, true, EMULATOR, "domios-d9357"),
        (true, true, EMULATOR, "domios-signin-qa"),
        (true, true, EMULATOR, "other-project"),
        (true, false, GOOGLE, "other-project"),
        (true, false, "http://firestore.googleapis.com", "domios-d9357"),
        (true, true, "http://localhost:18080", "demo-domios"),
        (true, true, "http://127.0.0.2:18080", "demo-domios"),
        (true, true, "http://[::1]:18080", "demo-domios"),
        (true, true, "http://127.0.0.1:8080", "demo-domios"),
        (true, true, "https://127.0.0.1:18080", "demo-domios"),
        (true, true, "http://user:secret@127.0.0.1:18080", "demo-domios"),
        (true, true, "http://127.0.0.1:18080/", "demo-domios"),
        (true, true, "http://127.0.0.1:18080?token=opaque", "demo-domios"),
        (true, true, "http://127.0.0.1:18080#opaque", "demo-domios"),
    ] {
        assert_eq!(FirestoreTarget::resolve(debug, selected, uri, project), Err(Error::Transport));
    }
}

struct Calls {
    target: Result<FirestoreTarget>,
    events: Arc<Mutex<Vec<&'static str>>>,
    tokens: Mutex<std::collections::VecDeque<Result<&'static str>>>,
}

#[async_trait::async_trait]
impl AuthenticatedCall for Calls {
    fn firestore_target(&self, _: &str) -> Result<FirestoreTarget> {
        self.events.lock().unwrap().push("target");
        self.target.clone()
    }

    async fn id_token(&self) -> Result<Zeroizing<String>> {
        self.events.lock().unwrap().push("token");
        self.tokens.lock().unwrap().pop_front().expect("bounded token port")
            .map(|token| Zeroizing::new(token.into()))
    }

    async fn invoke(&self, _: &str, _: serde_json::Value) -> Result<serde_json::Value> {
        panic!("Firestore construction must not invoke a callable")
    }
}

fn fixture(target: Result<FirestoreTarget>, project: &str, tokens: Vec<Result<&'static str>>) -> FirebaseTransport {
    FirebaseTransport {
        calls: Arc::new(Calls {
            target,
            events: Default::default(),
            tokens: Mutex::new(tokens.into()),
        }),
        project: project.into(),
        uid: "fixture-owner".into(),
        installation_id: "fixture-installation".into(),
    }
}

#[tokio::test]
async fn injected_channel_uses_exact_policy_and_account_metadata() {
    for target in [
        FirestoreTarget::google("domios-d9357").unwrap(),
        FirestoreTarget::google("domios-signin-qa").unwrap(),
        FirestoreTarget::resolve(true, true, EMULATOR, "demo-domios").unwrap(),
    ] {
        let events = Arc::new(Mutex::new(Vec::new()));
        let transport = FirebaseTransport {
            calls: Arc::new(Calls {
                target: Ok(target.clone()),
                events: events.clone(),
                tokens: Mutex::new([Ok("earlier-account-token"), Ok("current-account-token")].into()),
            }),
            ..fixture(Err(Error::Transport), target.project(), vec![])
        };
        let (channel, token, database) = super::super::connection_with(&transport, |selected| {
            events.lock().unwrap().push("channel");
            let expected = target.clone();
            async move {
                assert_eq!(selected, expected);
                Ok(17)
            }
        }).await.unwrap();
        assert_eq!(channel, 17);
        assert_eq!(&**token, "current-account-token");
        assert_eq!(database, target.database());
        let request = super::super::authenticated_request(&token, &database, ()).unwrap();
        let auth = request.metadata().get("authorization").unwrap();
        assert_eq!(auth.to_str().unwrap(), "Bearer current-account-token");
        assert!(auth.is_sensitive());
        assert_eq!(*events.lock().unwrap(), ["target", "token", "channel", "token", "target"]);
    }
}

#[tokio::test]
async fn invalid_or_mixed_target_refuses_before_channel_or_credentials() {
    for transport in [
        fixture(Err(Error::Transport), "demo-domios", vec![]),
        fixture(Ok(FirestoreTarget::google("domios-d9357").unwrap()), "demo-domios", vec![]),
    ] {
        let result = super::super::connection_with(&transport, |_| async {
            Err::<(), _>(Error::Unavailable)
        }).await;
        assert!(matches!(result, Err(Error::Transport)));
    }
    struct GoogleOnly;
    #[async_trait::async_trait]
    impl AuthenticatedCall for GoogleOnly {
        async fn id_token(&self) -> Result<Zeroizing<String>> { panic!("unexpected credential use") }
        async fn invoke(&self, _: &str, _: serde_json::Value) -> Result<serde_json::Value> { panic!("unexpected callable") }
    }
    assert_eq!(GoogleOnly.firestore_target("demo-domios"), Err(Error::Transport));
}

#[tokio::test]
async fn all_document_factories_refuse_unsupported_target_before_token_use() {
    let transport = fixture(Err(Error::Transport), "demo-domios", vec![]);
    let friendship = "a".repeat(64);
    assert!(matches!(transport.listen_state().await, Err(Error::Transport)));
    assert!(matches!(transport.listen_registry().await, Err(Error::Transport)));
    assert!(matches!(transport.listen_from_cursor(&friendship, None).await, Err(Error::Transport)));
    assert!(matches!(transport.listen_pending(&friendship).await, Err(Error::Transport)));
    assert_eq!(transport.pending_count(&friendship).await, Err(Error::Transport));
}

#[tokio::test]
async fn revoked_session_refuses_before_dial_and_after_injected_channel_without_replay() {
    for tokens in [vec![Err(Error::Revoked)], vec![Ok("own-account-token"), Err(Error::Revoked)]] {
        let expected_connects = usize::from(tokens.len() == 2);
        let transport = fixture(Ok(FirestoreTarget::google("domios-d9357").unwrap()), "domios-d9357", tokens);
        let connects = std::sync::atomic::AtomicUsize::new(0);
        let result = super::super::connection_with(&transport, |_| async {
            connects.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }).await;
        assert!(matches!(result, Err(Error::Revoked)));
        assert_eq!(connects.load(std::sync::atomic::Ordering::SeqCst), expected_connects);
    }
}
