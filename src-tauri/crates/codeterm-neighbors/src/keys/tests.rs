use super::*;
use rand::{rngs::StdRng, SeedableRng};
use std::{collections::HashMap, sync::Mutex};

mod registration;

#[derive(Default)]
struct Store(Mutex<HashMap<String, Zeroizing<Vec<u8>>>>);

#[async_trait]
impl KeyStore for Store {
    async fn load(&self, scope: &KeyScope, slot: Slot) -> Result<Option<Zeroizing<Vec<u8>>>> {
        Ok(self.0.lock().unwrap().get(&scope.name(slot)).cloned())
    }
    async fn save(&self, scope: &KeyScope, slot: Slot, bytes: Zeroizing<Vec<u8>>) -> Result<()> {
        self.0.lock().unwrap().insert(scope.name(slot), bytes);
        Ok(())
    }
    async fn delete(&self, scope: &KeyScope, slot: Slot) -> Result<()> {
        self.0.lock().unwrap().remove(&scope.name(slot));
        Ok(())
    }
}

#[derive(Default)]
struct Registry {
    active: Mutex<Vec<String>>,
    fail_enroll: Mutex<bool>,
}

#[async_trait]
impl KeyRegistry for Registry {
    async fn enroll(&self, signing: &str, _: &str) -> Result<String> {
        if *self.fail_enroll.lock().unwrap() {
            return Err(Error::Transport);
        }
        let id = format!(
            "vk_{}",
            &sha256_hex(format!("verifier-key:owner:{signing}").as_bytes())[..32]
        );
        let mut active = self.active.lock().unwrap();
        if !active.contains(&id) {
            active.push(id.clone());
        }
        assert!(active.len() <= 2);
        Ok(id)
    }
    async fn revoke(&self, id: &str) -> Result<()> {
        self.active.lock().unwrap().retain(|key| key != id);
        Ok(())
    }
    async fn active_key_ids(&self) -> Result<Vec<String>> {
        Ok(self.active.lock().unwrap().clone())
    }
}

#[tokio::test]
async fn installation_keys_survive_gateway_recreation_rotate_and_revoke() {
    let store = Store::default();
    let registry = Registry::default();
    let lifecycle = InstallationKeys {
        store: &store,
        registry: &registry,
        scope: KeyScope::installation("machine", "owner").unwrap(),
        uid: "owner",
    };
    let mut rng = StdRng::seed_from_u64(8);
    let first = lifecycle.enroll(&mut rng).await.unwrap();
    let reopened = lifecycle.enroll(&mut rng).await.unwrap();
    assert_eq!(first.encryption_key_id(), reopened.encryption_key_id());
    let rotated = lifecycle.rotate(&mut rng).await.unwrap();
    assert_ne!(first.encryption_key_id(), rotated.encryption_key_id());
    assert_eq!(
        *registry.active.lock().unwrap(),
        [rotated.verifier_key_id("owner")]
    );
    lifecycle.revoke().await.unwrap();
    assert!(lifecycle.load().await.unwrap().is_none());
    assert!(registry.active.lock().unwrap().is_empty());
    assert!(store.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn failed_enrollment_resumes_the_persisted_candidate() {
    let store = Store::default();
    let registry = Registry::default();
    let lifecycle = InstallationKeys {
        store: &store,
        registry: &registry,
        scope: KeyScope::installation("machine", "owner").unwrap(),
        uid: "owner",
    };
    let mut rng = StdRng::seed_from_u64(4);
    *registry.fail_enroll.lock().unwrap() = true;
    assert!(matches!(
        lifecycle.enroll(&mut rng).await,
        Err(Error::Transport)
    ));
    let candidate = store
        .load(&lifecycle.scope, Slot::Candidate)
        .await
        .unwrap()
        .unwrap();
    let expected = PrivateKeys::from_storage(&candidate)
        .unwrap()
        .encryption_key_id();
    *registry.fail_enroll.lock().unwrap() = false;
    assert_eq!(
        lifecycle
            .enroll(&mut rng)
            .await
            .unwrap()
            .encryption_key_id(),
        expected
    );
    assert!(store
        .load(&lifecycle.scope, Slot::Candidate)
        .await
        .unwrap()
        .is_none());
}

#[test]
fn key_scope_binds_installation_and_account_without_exporting_ids() {
    let a = KeyScope::installation("machine", "alice").unwrap();
    let b = KeyScope::installation("machine", "bob").unwrap();
    let c = KeyScope::installation("other", "alice").unwrap();
    assert_ne!(a.name(Slot::Current), b.name(Slot::Current));
    assert_ne!(a.name(Slot::Current), c.name(Slot::Current));
    assert!(!a.name(Slot::Current).contains("alice"));
}

#[tokio::test]
async fn rotation_recovery_preserves_keys_owned_by_another_installation() {
    let store = Store::default();
    let registry = Registry::default();
    let lifecycle = InstallationKeys {
        store: &store,
        registry: &registry,
        scope: KeyScope::installation("machine", "owner").unwrap(),
        uid: "owner",
    };
    let mut rng = StdRng::seed_from_u64(23);
    let old = PrivateKeys::generate(&mut rng);
    let candidate = PrivateKeys::generate(&mut rng);
    let foreign = PrivateKeys::generate(&mut rng).verifier_key_id("owner");
    store
        .save(&lifecycle.scope, Slot::Current, old.storage_bytes())
        .await
        .unwrap();
    store
        .save(&lifecycle.scope, Slot::Candidate, candidate.storage_bytes())
        .await
        .unwrap();
    *registry.active.lock().unwrap() = vec![
        old.verifier_key_id("owner"),
        candidate.verifier_key_id("owner"),
        foreign.clone(),
    ];
    lifecycle.reconcile().await.unwrap().unwrap();
    assert_eq!(
        *registry.active.lock().unwrap(),
        [candidate.verifier_key_id("owner"), foreign]
    );
}

#[tokio::test]
async fn revocation_preserves_keys_owned_by_another_installation() {
    let store = Store::default();
    let registry = Registry::default();
    let lifecycle = InstallationKeys {
        store: &store,
        registry: &registry,
        scope: KeyScope::installation("machine", "owner").unwrap(),
        uid: "owner",
    };
    let mut rng = StdRng::seed_from_u64(18);
    let local = lifecycle.enroll(&mut rng).await.unwrap();
    let foreign = PrivateKeys::generate(&mut rng).verifier_key_id("owner");
    registry.active.lock().unwrap().push(foreign.clone());
    lifecycle.revoke().await.unwrap();
    assert_eq!(*registry.active.lock().unwrap(), vec![foreign]);
    assert!(lifecycle.load().await.unwrap().is_none());
    assert!(!registry
        .active
        .lock()
        .unwrap()
        .contains(&local.verifier_key_id("owner")));
}
