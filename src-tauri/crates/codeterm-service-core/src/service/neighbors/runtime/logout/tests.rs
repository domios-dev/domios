use super::*;
use async_trait::async_trait;
use codeterm_account::{CredentialStore, IdentityProvider};
use codeterm_neighbors::{adapter::AuthenticatedCall, crypto::PrivateKeys, keys::Slot};
use parking_lot::Mutex;
use rand::{rngs::StdRng, SeedableRng};
use std::{collections::HashMap, time::Duration};

#[derive(Default)]
struct Evidence {
    events: Mutex<Vec<&'static str>>,
    active: Mutex<Vec<String>>,
    slots: Mutex<HashMap<String, Zeroizing<Vec<u8>>>>,
    credential: Mutex<Option<codeterm_account::Secret>>,
    fail_delete: Mutex<bool>,
    uncertain: bool,
    uncertain_confirmation: bool,
}

#[async_trait]
impl CredentialStore for Evidence {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<codeterm_account::Secret>> {
        Ok(self.credential.lock().clone())
    }
    async fn save(
        &self,
        _: &str,
        token: &codeterm_account::Secret,
    ) -> codeterm_account::Result<()> {
        *self.credential.lock() = Some(token.clone());
        Ok(())
    }
    async fn delete(&self, _: &str) -> codeterm_account::Result<()> {
        self.events.lock().push("delete_credential");
        *self.credential.lock() = None;
        Ok(())
    }
}

#[async_trait]
impl KeyStore for Evidence {
    async fn load(
        &self,
        scope: &KeyScope,
        slot: Slot,
    ) -> codeterm_neighbors::Result<Option<Zeroizing<Vec<u8>>>> {
        self.events.lock().push("load_key");
        Ok(self.slots.lock().get(&scope.name(slot)).cloned())
    }
    async fn save(
        &self,
        scope: &KeyScope,
        slot: Slot,
        bytes: Zeroizing<Vec<u8>>,
    ) -> codeterm_neighbors::Result<()> {
        self.slots.lock().insert(scope.name(slot), bytes);
        Ok(())
    }
    async fn delete(&self, scope: &KeyScope, slot: Slot) -> codeterm_neighbors::Result<()> {
        if *self.fail_delete.lock() {
            return Err(Error::SecretStore);
        }
        self.events.lock().push("delete_key");
        self.slots.lock().remove(&scope.name(slot));
        Ok(())
    }
}

#[async_trait]
impl AuthenticatedCall for Evidence {
    async fn invoke(&self, route: &str, data: Value) -> codeterm_neighbors::Result<Value> {
        match route {
            "friendsOwnKeys" => {
                self.events.lock().push("inventory");
                if self.uncertain || self.uncertain_confirmation && self.active.lock().len() == 1 {
                    return Err(Error::Transport);
                }
                let ids = self.active.lock().clone();
                if ids.len() <= 1 {
                    self.events.lock().push("confirmed_local_absent");
                }
                Ok(json!(ids
                    .iter()
                    .map(|id| json!({"key_id":id}))
                    .collect::<Vec<_>>()))
            }
            "friendsRevokeVerifierKey" => {
                self.events.lock().push("revoke");
                self.active
                    .lock()
                    .retain(|id| Some(id.as_str()) != data["verifier_key_id"].as_str());
                Ok(json!({}))
            }
            _ => panic!("logout attempted a non-key callable: {route}"),
        }
    }
    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
        panic!("logout must not construct or authenticate a native listener")
    }
}

struct Provider(bool);
struct NoBrowser;
#[async_trait]
impl codeterm_account::Browser for NoBrowser {
    async fn open(&self, _: &url::Url) -> codeterm_account::Result<()> {
        panic!("no browser")
    }
}
#[async_trait]
impl IdentityProvider for Provider {
    fn needs_browser_callback(&self) -> bool {
        false
    }
    fn id(&self) -> &str {
        if self.0 {
            "local"
        } else {
            "firebase"
        }
    }
    async fn start_login(
        &self,
        _: &codeterm_account::LoginFlow,
    ) -> codeterm_account::Result<codeterm_account::LoginStart> {
        self.refresh(&codeterm_account::Secret::new("fresh-sign-in"))
            .await
            .map(codeterm_account::LoginStart::Immediate)
    }
    async fn redeem(
        &self,
        _: codeterm_account::Redemption,
    ) -> codeterm_account::Result<codeterm_account::Tokens> {
        Err(codeterm_account::Error::Provider)
    }
    async fn refresh(
        &self,
        _: &codeterm_account::Secret,
    ) -> codeterm_account::Result<codeterm_account::Tokens> {
        Ok(codeterm_account::Tokens {
            session: codeterm_account::Session {
                login_generation: 0,
                identity: codeterm_account::Identity {
                    uid: "owner".into(),
                    display_name: None,
                    email: None,
                    provider: self.id().into(),
                    photo_url: None,
                },
                id_token: codeterm_account::Secret::new("opaque-session"),
                expires_at: None,
            },
            refresh_token: codeterm_account::Secret::new("opaque-credential"),
        })
    }
    async fn revoke(&self, _: &codeterm_account::Secret) -> codeterm_account::Result<()> {
        Ok(())
    }
}

struct Enrollment(String);

#[async_trait]
impl KeyRegistry for Enrollment {
    async fn enroll(&self, _: &str, _: &str, _: bool) -> codeterm_neighbors::Result<String> {
        Ok(self.0.clone())
    }

    async fn revoke(&self, _: &str) -> codeterm_neighbors::Result<()> {
        panic!("fixture enrollment must not revoke another key")
    }

    async fn active_key_ids(&self) -> codeterm_neighbors::Result<Vec<String>> {
        Ok(vec![self.0.clone()])
    }
}

async fn logout(
    local: bool,
    enrolled: bool,
    uncertain: bool,
    uncertain_confirmation: bool,
    self_sign_out: bool,
) {
    use codeterm_billing::GrantStore;
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    let grants = codeterm_billing::FileGrantStore::new(service.account_data_root().unwrap());
    grants.save(&json!({"opaque":"cached-grant"})).unwrap();
    grants.save_hint(true).unwrap();
    let evidence = Arc::new(Evidence {
        uncertain,
        uncertain_confirmation,
        ..Default::default()
    });
    *evidence.credential.lock() = Some(codeterm_account::Secret::new("opaque-credential"));
    let account = Arc::new(codeterm_account::Account::new(
        Arc::new(Provider(local)),
        evidence.clone(),
        Duration::from_secs(1),
    ));
    account.refresh().await.unwrap();
    service
        .ext::<crate::service::account::AccountCell>()
        .0
        .set(account.clone())
        .unwrap_or_else(|_| panic!("account initialized"));
    if enrolled {
        let installation_id = service.installation_id().await.unwrap();
        let scope = KeyScope::installation(installation_id.as_str(), "owner").unwrap();
        let mut rng = StdRng::seed_from_u64(42);
        let enrollment =
            Enrollment(PrivateKeys::generate(&mut rng.clone()).verifier_key_id("owner"));
        let keys = InstallationKeys {
            store: evidence.as_ref(),
            registry: &enrollment,
            scope: scope.clone(),
            uid: "owner",
        }
        .enroll(true, &mut rng)
        .await
        .unwrap();
        let current = KeyStore::load(evidence.as_ref(), &scope, Slot::Current)
            .await
            .unwrap()
            .unwrap();
        KeyStore::save(evidence.as_ref(), &scope, Slot::Candidate, current)
            .await
            .unwrap();
        evidence.active.lock().push(keys.verifier_key_id("owner"));
        let inventory_only = PrivateKeys::generate(&mut rng);
        evidence
            .active
            .lock()
            .push(inventory_only.verifier_key_id("owner"));
        evidence.events.lock().clear();
        assert!(
            !Metadata::new(service.account_data_root().unwrap(), "owner")
                .settings()
                .await
                .unwrap()
                .enabled
        );
    }
    service
        .set_ext(LogoutKeyPorts {
            store: evidence.clone(),
            registry: Arc::new(FirebaseRegistry {
                calls: evidence.clone(),
                uid: "owner".into(),
                installation_id: "machine".into(),
                machine_label: "Fixture".into(),
                enrollment: None,
            }),
        })
        .ok()
        .unwrap();
    let result = if self_sign_out {
        let outcome = service.account_mesh_session_revoke(None).await.unwrap();
        assert!(matches!(
            outcome,
            crate::service::account_mesh::AccountMeshOutcome::Refused {
                local_signed_out: true,
                ..
            }
        ));
        account.status().await
    } else {
        service.account_logout().await.unwrap()
    };
    assert!(result.identity.is_none());
    assert!(account.session().await.is_none());
    assert!(evidence.credential.lock().is_none());
    assert!(grants.load().unwrap().is_none());
    assert!(!grants.load_hint().unwrap());
    assert_eq!(*evidence.events.lock(), ["delete_credential"]);
    assert_eq!(evidence.slots.lock().len(), if enrolled { 2 } else { 0 });
    let calls = callable::Calls {
        service: service.clone(),
        uid: "owner".into(),
        generation: 0,
    };
    assert!(matches!(calls.id_token().await, Err(Error::Revoked)));
    assert_eq!(*evidence.events.lock(), ["delete_credential"]);
    if local {
        return;
    }
    let metadata = Metadata::new(service.account_data_root().unwrap(), "owner");
    assert!(metadata.settings().await.unwrap().cleanup_pending);
    assert!(!metadata.settings().await.unwrap().enabled);
    account.login(Arc::new(NoBrowser)).await.unwrap();
    let scope =
        KeyScope::installation(service.installation_id().await.unwrap().as_str(), "owner").unwrap();
    let ports = service.ext::<LogoutKeyPorts>();
    let active_before = evidence.active.lock().clone();
    let retry = service
        .retry_neighbor_logout(&metadata, &scope, "owner", ports.registry.as_ref())
        .await;
    assert!(retry.is_ok());
    assert!(!metadata.settings().await.unwrap().cleanup_pending);
    assert!(evidence.slots.lock().is_empty());
    assert!(!evidence
        .events
        .lock()
        .iter()
        .any(|event| matches!(*event, "inventory" | "revoke")));
    assert_eq!(*evidence.active.lock(), active_before);
}

#[tokio::test]
async fn local_account_logout_does_not_use_neighbor_ports() {
    logout(true, false, false, false, false).await;
}
#[tokio::test]
async fn never_enrolled_logout_clears_only_local_slots() {
    logout(false, false, false, false, false).await;
}
#[tokio::test]
async fn logout_cleanup_keeps_another_installations_keys() {
    logout(false, true, false, false, false).await;
}
#[tokio::test]
async fn local_cleanup_does_not_require_inventory_after_local_sign_out() {
    logout(false, true, true, false, false).await;
}
#[tokio::test]
async fn local_cleanup_does_not_require_revocation_after_local_sign_out() {
    logout(false, true, false, true, false).await;
}

#[tokio::test]
async fn self_session_revocation_keeps_local_sign_out_when_backend_is_unavailable() {
    logout(false, true, false, false, true).await;
}

#[tokio::test]
async fn failed_local_key_cleanup_remains_pending_until_the_store_accepts_it() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
    let evidence = Arc::new(Evidence::default());
    *evidence.fail_delete.lock() = true;
    service
        .set_ext(LogoutKeyPorts {
            store: evidence.clone(),
            registry: Arc::new(Enrollment("unused".into())),
        })
        .ok()
        .unwrap();
    let metadata = Metadata::new(service.account_data_root().unwrap(), "owner");
    let mut settings = metadata.settings().await.unwrap();
    settings.cleanup_pending = true;
    metadata.save_settings(settings).await.unwrap();
    let scope = KeyScope::installation("machine", "owner").unwrap();
    let registry = Enrollment("unused".into());
    assert!(service
        .retry_neighbor_logout(&metadata, &scope, "owner", &registry)
        .await
        .is_err());
    assert!(metadata.settings().await.unwrap().cleanup_pending);
    *evidence.fail_delete.lock() = false;
    service
        .retry_neighbor_logout(&metadata, &scope, "owner", &registry)
        .await
        .unwrap();
    assert!(!metadata.settings().await.unwrap().cleanup_pending);
}
