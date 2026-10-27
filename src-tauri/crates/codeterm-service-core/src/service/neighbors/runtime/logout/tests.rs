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
        assert!(self.active.lock().is_empty());
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
                if self.uncertain || self.uncertain_confirmation && self.active.lock().is_empty() {
                    return Err(Error::Transport);
                }
                let ids = self.active.lock().clone();
                if ids.is_empty() {
                    self.events.lock().push("confirmed_empty");
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
#[async_trait]
impl IdentityProvider for Provider {
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
        Err(codeterm_account::Error::Provider)
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

async fn logout(local: bool, enrolled: bool, uncertain: bool, uncertain_confirmation: bool) {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let service = &fixture.service;
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
        let scope =
            KeyScope::installation(&service.installation_id().await.unwrap(), "owner").unwrap();
        let mut rng = StdRng::seed_from_u64(42);
        for slot in [Slot::Current, Slot::Candidate] {
            let keys = PrivateKeys::generate(&mut rng);
            evidence.active.lock().push(keys.verifier_key_id("owner"));
            KeyStore::save(evidence.as_ref(), &scope, slot, keys.storage_bytes())
                .await
                .unwrap();
        }
        let inventory_only = PrivateKeys::generate(&mut rng);
        evidence
            .active
            .lock()
            .push(inventory_only.verifier_key_id("owner"));
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
            }),
        })
        .ok()
        .unwrap();
    let result = service.account_logout().await;
    let should_refuse = !local && (uncertain || uncertain_confirmation);
    assert_eq!(result.is_err(), should_refuse);
    assert_eq!(account.session().await.is_some(), should_refuse);
    assert_eq!(evidence.credential.lock().is_some(), should_refuse);
    let events = evidence.events.lock();
    if should_refuse {
        assert!(!events.contains(&"delete_key"));
        assert!(!events.contains(&"delete_credential"));
        assert_eq!(evidence.slots.lock().len(), 2);
    } else if local {
        assert_eq!(*events, ["delete_credential"]);
    } else {
        let confirmed = events
            .iter()
            .position(|event| *event == "confirmed_empty")
            .unwrap();
        let deleted = events
            .iter()
            .position(|event| *event == "delete_credential")
            .unwrap();
        assert!(confirmed < deleted);
        if enrolled {
            assert!(
                events
                    .iter()
                    .position(|event| *event == "delete_key")
                    .unwrap()
                    > confirmed
            );
            assert!(evidence.slots.lock().is_empty());
        } else {
            assert!(!events.contains(&"load_key"));
            assert!(!events.contains(&"delete_key"));
        }
    }
}

#[tokio::test]
async fn local_account_logout_does_not_use_neighbor_ports() {
    logout(true, false, false, false).await;
}
#[tokio::test]
async fn never_enrolled_logout_confirms_inventory_without_opening_secret_store() {
    logout(false, false, false, false).await;
}
#[tokio::test]
async fn emulator_logout_uses_callable_registry_and_deletes_only_after_confirmed_revocation() {
    logout(false, true, false, false).await;
}
#[tokio::test]
async fn uncertain_inventory_preserves_account_credentials_and_both_key_slots() {
    logout(false, true, true, false).await;
}
#[tokio::test]
async fn uncertain_revocation_confirmation_preserves_credentials_and_both_key_slots() {
    logout(false, true, false, true).await;
}
