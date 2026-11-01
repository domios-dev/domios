use super::*;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
struct MockPorts {
    persisted: Mutex<Vec<Intent>>,
    offline: Mutex<bool>,
    receiver_enabled: Mutex<bool>,
    revoked: Mutex<Vec<String>>,
    cutoffs: Mutex<Vec<(String, u64)>>,
}

#[async_trait]
impl StopPorts for MockPorts {
    async fn load(&self) -> Result<Vec<Intent>, AccountMeshRefusal> {
        Ok(self.persisted.lock().clone())
    }

    async fn save(&self, intents: &[Intent]) -> Result<(), AccountMeshRefusal> {
        *self.persisted.lock() = intents.to_vec();
        Ok(())
    }

    async fn revoke(&self, session: &Session) -> Result<SessionRevokedResult, AccountMeshRefusal> {
        self.revoked
            .lock()
            .push(session.id_token.expose().to_owned());
        if *self.offline.lock() {
            return Err(AccountMeshRefusal::Offline);
        }
        *self.receiver_enabled.lock() = false;
        Ok(SessionRevokedResult {
            target_installation_id: "desk-7f3a".into(),
            revoked_at: 120,
        })
    }

    async fn revoke_before(
        &self,
        session: &Session,
        cutoff: u64,
    ) -> Result<(), AccountMeshRefusal> {
        self.cutoffs
            .lock()
            .push((session.id_token.expose().to_owned(), cutoff));
        if *self.offline.lock() {
            Err(AccountMeshRefusal::Offline)
        } else {
            *self.receiver_enabled.lock() = false;
            Ok(())
        }
    }
}

fn session() -> Session {
    Session {
        identity: Identity {
            uid: "owner".into(),
            email: None,
            display_name: None,
            provider: "fixture".into(),
            photo_url: None,
        },
        login_generation: 3,
        id_token: Secret::new(format!(
            "h.{}.s",
            URL_SAFE_NO_PAD.encode(r#"{"auth_time":50}"#)
        )),
        expires_at: Some(200),
    }
}

#[tokio::test]
async fn online_sign_out_revokes_once_and_clears_the_intent() {
    let ports = MockPorts::default();
    *ports.receiver_enabled.lock() = true;
    let captured = session();
    let result = capture_and_revoke(&ports, &captured, 100)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.target_installation_id, "desk-7f3a");
    assert_eq!(&*ports.revoked.lock(), &[captured.id_token.expose()]);
    assert!(ports.persisted.lock().is_empty());
    assert!(!*ports.receiver_enabled.lock());
    retry(&ports, 101, None).await.unwrap();
    assert_eq!(ports.revoked.lock().len(), 1);
}

#[tokio::test]
async fn offline_sign_out_reconnect_completes_from_persisted_intent_without_a_session() {
    let ports = MockPorts::default();
    *ports.receiver_enabled.lock() = true;
    *ports.offline.lock() = true;
    assert_eq!(
        capture_and_revoke(&ports, &session(), 100).await,
        Err(AccountMeshRefusal::Offline)
    );
    assert_eq!(ports.persisted.lock().len(), 1);
    assert!(*ports.receiver_enabled.lock());
    let stored = serde_json::to_string(&*ports.persisted.lock()).unwrap();
    assert!(!stored.contains("refresh"));
    let restarted = MockPorts::default();
    *restarted.receiver_enabled.lock() = true;
    *restarted.persisted.lock() = serde_json::from_str(&stored).unwrap();
    retry(&restarted, 150, None).await.unwrap();
    assert_eq!(&*restarted.revoked.lock(), &[session().id_token.expose()]);
    assert!(restarted.persisted.lock().is_empty());
    assert!(!*restarted.receiver_enabled.lock());
    assert!(restarted.cutoffs.lock().is_empty());
}

#[tokio::test]
async fn expired_capture_is_removed_and_new_sign_in_uses_its_auth_time_cutoff() {
    let ports = MockPorts::default();
    *ports.offline.lock() = true;
    let _ = capture_and_revoke(&ports, &session(), 100).await;
    assert_eq!(
        retry(&ports, 200, None).await,
        Err(AccountMeshRefusal::Unauthenticated)
    );
    assert!(ports.persisted.lock()[0].id_token.is_none());
    assert_eq!(ports.revoked.lock().len(), 1);
    assert_eq!(
        retry(&ports, 201, Some(&session())).await,
        Err(AccountMeshRefusal::ReauthRequired)
    );
    let mut fresh = session();
    fresh.id_token = Secret::new(format!(
        "h.{}.s",
        URL_SAFE_NO_PAD.encode(r#"{"auth_time":300}"#)
    ));
    assert_eq!(
        retry(&ports, 301, Some(&fresh)).await,
        Err(AccountMeshRefusal::Offline)
    );
    assert_eq!(ports.persisted.lock().len(), 1);
    *ports.offline.lock() = false;
    retry(&ports, 302, Some(&fresh)).await.unwrap();
    assert!(ports.persisted.lock().is_empty());
    assert_eq!(ports.cutoffs.lock()[0].1, 300);
    assert_eq!(ports.revoked.lock().len(), 1);
}

struct Admission {
    ports: Arc<MockPorts>,
    now: AtomicU64,
}

#[async_trait]
impl codeterm_account::SessionAdmission for Admission {
    async fn admit(&self, session: &Session) -> codeterm_account::Result<()> {
        retry(
            self.ports.as_ref(),
            self.now.load(Ordering::SeqCst),
            Some(session),
        )
        .await
        .map_err(|_| codeterm_account::Error::Transport)
    }
}

#[async_trait]
impl codeterm_account::IdentityObserver for Admission {
    async fn changed(&self, identity: Option<&Identity>) {
        if let Some(identity) = identity {
            assert!(
                !self
                    .ports
                    .persisted
                    .lock()
                    .iter()
                    .any(|intent| intent.identity.uid == identity.uid),
                "this account's server stop must complete before publication"
            );
        }
    }
}

struct Provider(Session);

#[async_trait]
impl codeterm_account::IdentityProvider for Provider {
    fn id(&self) -> &str {
        "fixture"
    }
    fn needs_browser_callback(&self) -> bool {
        false
    }
    async fn start_login(
        &self,
        _: &codeterm_account::LoginFlow,
    ) -> codeterm_account::Result<codeterm_account::LoginStart> {
        Ok(codeterm_account::LoginStart::Immediate(
            codeterm_account::Tokens {
                session: self.0.clone(),
                refresh_token: Secret::new("new-refresh"),
            },
        ))
    }
    async fn redeem(
        &self,
        _: codeterm_account::Redemption,
    ) -> codeterm_account::Result<codeterm_account::Tokens> {
        Err(codeterm_account::Error::Provider)
    }
    async fn refresh(&self, _: &Secret) -> codeterm_account::Result<codeterm_account::Tokens> {
        panic!("admission must not refresh either account")
    }
    async fn revoke(&self, _: &Secret) -> codeterm_account::Result<()> {
        panic!("sign-out must not use a refresh token for revocation")
    }
}

#[derive(Default)]
struct Credentials(Mutex<Option<Secret>>);

#[async_trait]
impl codeterm_account::CredentialStore for Credentials {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<Secret>> {
        Ok(self.0.lock().clone())
    }
    async fn save(&self, _: &str, token: &Secret) -> codeterm_account::Result<()> {
        *self.0.lock() = Some(token.clone());
        Ok(())
    }
    async fn delete(&self, _: &str) -> codeterm_account::Result<()> {
        *self.0.lock() = None;
        Ok(())
    }
}

struct NoBrowser;

#[async_trait]
impl codeterm_account::Browser for NoBrowser {
    async fn open(&self, _: &url::Url) -> codeterm_account::Result<()> {
        panic!("injected immediate login")
    }
}

fn account(session: Session, admission: Arc<Admission>) -> Arc<codeterm_account::Account> {
    Arc::new(
        codeterm_account::Account::new(
            Arc::new(Provider(session)),
            Arc::new(Credentials::default()),
            Duration::from_secs(1),
        )
        .with_session_admission(admission.clone())
        .with_identity_observer(admission),
    )
}

#[tokio::test]
async fn expired_account_a_intent_allows_b_login_and_still_fences_a_publication_after_restart() {
    let ports = Arc::new(MockPorts::default());
    let admission = Arc::new(Admission {
        ports: ports.clone(),
        now: AtomicU64::new(100),
    });
    let original_a = account(session(), admission.clone());
    original_a.login(Arc::new(NoBrowser)).await.unwrap();
    let captured_a = original_a.session().await.unwrap();
    original_a.logout().await.unwrap();
    *ports.offline.lock() = true;
    assert_eq!(
        capture_and_revoke(ports.as_ref(), &captured_a, 100).await,
        Err(AccountMeshRefusal::Offline)
    );

    let persisted = serde_json::to_string(&*ports.persisted.lock()).unwrap();
    let restarted_ports = Arc::new(MockPorts::default());
    *restarted_ports.persisted.lock() = serde_json::from_str(&persisted).unwrap();
    let admission = Arc::new(Admission {
        ports: restarted_ports.clone(),
        now: AtomicU64::new(501),
    });
    let mut fresh_b = session();
    fresh_b.identity.uid = "account-b".into();
    fresh_b.expires_at = Some(u64::MAX);
    let b = account(fresh_b, admission.clone());
    assert_eq!(
        b.login(Arc::new(NoBrowser))
            .await
            .unwrap()
            .identity
            .unwrap()
            .uid,
        "account-b"
    );
    assert_eq!(restarted_ports.persisted.lock().len(), 1);
    assert_eq!(
        restarted_ports.persisted.lock()[0].identity.uid,
        captured_a.identity.uid
    );
    assert!(restarted_ports.persisted.lock()[0].id_token.is_none());
    assert!(restarted_ports.cutoffs.lock().is_empty());
    drop(b);

    let mut fresh_a = session();
    fresh_a.expires_at = Some(u64::MAX);
    fresh_a.id_token = Secret::new(format!(
        "h.{}.s",
        URL_SAFE_NO_PAD.encode(r#"{"auth_time":400}"#)
    ));
    let a = account(fresh_a.clone(), admission);
    assert!(a.session().await.is_none());
    assert_eq!(
        a.login(Arc::new(NoBrowser))
            .await
            .unwrap()
            .identity
            .unwrap()
            .uid,
        captured_a.identity.uid
    );
    assert_eq!(
        &*restarted_ports.cutoffs.lock(),
        &[(fresh_a.id_token.expose().to_owned(), 400)]
    );
    assert!(restarted_ports.persisted.lock().is_empty());
    assert!(original_a.session().await.is_none());
}

#[tokio::test]
async fn expired_head_intent_does_not_starve_executable_reconnect_cleanup() {
    let ports = MockPorts::default();
    let mut b = session();
    b.identity.uid = "account-b".into();
    b.id_token = Secret::new("account-b-current-token");
    b.expires_at = Some(500);
    *ports.persisted.lock() = vec![Intent::captured(&session(), 100), Intent::captured(&b, 250)];
    assert_eq!(
        retry(&ports, 300, None).await,
        Err(AccountMeshRefusal::Unauthenticated)
    );
    assert_eq!(&*ports.revoked.lock(), &[b.id_token.expose()]);
    assert_eq!(ports.persisted.lock().len(), 1);
    assert_eq!(ports.persisted.lock()[0].identity.uid, "owner");
    assert!(ports.persisted.lock()[0].id_token.is_none());
}
