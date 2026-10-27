use super::*;
use parking_lot::Mutex;

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
