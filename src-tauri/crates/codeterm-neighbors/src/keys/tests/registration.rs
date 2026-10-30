use super::*;

#[tokio::test]
async fn active_installation_key_is_reused_without_enrolling_again() {
    let store = Store::default();
    let mut rng = StdRng::seed_from_u64(29);
    let key = PrivateKeys::generate(&mut rng);
    let scope = KeyScope::installation("machine", "owner").unwrap();
    store
        .save(&scope, Slot::Current, key.storage_bytes())
        .await
        .unwrap();
    struct Existing(String);
    #[async_trait]
    impl KeyRegistry for Existing {
        async fn active_key_ids(&self) -> Result<Vec<String>> {
            Ok(vec![self.0.clone()])
        }
        async fn enroll(&self, _: &str, _: &str) -> Result<String> {
            panic!("key is already enrolled")
        }
        async fn revoke(&self, _: &str) -> Result<()> {
            panic!("no revocation")
        }
    }
    let registry = Existing(key.verifier_key_id("owner"));
    let lifecycle = InstallationKeys {
        store: &store,
        registry: &registry,
        scope,
        uid: "owner",
    };
    for _ in 0..2 {
        assert_eq!(
            lifecycle
                .enroll(&mut rng)
                .await
                .unwrap()
                .encryption_key_id(),
            key.encryption_key_id()
        );
    }
}

struct RecoveryRegistry {
    active: Mutex<HashMap<String, String>>,
    revoked: std::collections::HashSet<String>,
    requests: Mutex<Vec<String>>,
    transport: bool,
    reject_all: bool,
}

#[async_trait]
impl KeyRegistry for RecoveryRegistry {
    async fn enroll(&self, signing: &str, _: &str) -> Result<String> {
        let id = format!("vk_{}", &sha256_hex(format!("verifier-key:owner:{signing}").as_bytes())[..32]);
        self.requests.lock().unwrap().push(id.clone());
        if self.transport { return Err(Error::Transport); }
        if self.reject_all || self.revoked.contains(&id) { return Err(Error::Revoked); }
        self.active.lock().unwrap().insert("machine".into(), id.clone());
        Ok(id)
    }
    async fn revoke(&self, id: &str) -> Result<()> {
        self.active.lock().unwrap().retain(|_, active| active != id);
        Ok(())
    }
    async fn active_key_ids(&self) -> Result<Vec<String>> {
        Ok(self.active.lock().unwrap().values().cloned().collect())
    }
}

#[tokio::test]
async fn explicit_enrollment_recovers_only_a_definitively_revoked_candidate_with_one_fresh_key() {
    let mut rng = StdRng::seed_from_u64(71);
    let revoked = PrivateKeys::generate(&mut rng);
    let revoked_id = revoked.verifier_key_id("owner");
    let registry = RecoveryRegistry {
        active: Mutex::new(HashMap::from([("other-machine".into(), "vk_foreign".into())])),
        revoked: std::collections::HashSet::from([revoked_id.clone()]),
        requests: Mutex::default(), transport: false, reject_all: false,
    };
    let store = Store::default();
    let scope = KeyScope::installation("machine", "owner").unwrap();
    store.save(&scope, Slot::Candidate, revoked.storage_bytes()).await.unwrap();
    let lifecycle = InstallationKeys { store: &store, registry: &registry, scope: scope.clone(), uid: "owner" };
    let recovered = lifecycle.enroll(&mut rng).await.unwrap();
    assert_ne!(recovered.verifier_key_id("owner"), revoked_id);
    assert_eq!(*registry.requests.lock().unwrap(), vec![revoked_id, recovered.verifier_key_id("owner")]);
    assert_eq!(registry.active.lock().unwrap().len(), 2);
    assert_eq!(registry.active.lock().unwrap()["other-machine"], "vk_foreign");
    assert_eq!(lifecycle.load().await.unwrap().unwrap().encryption_key_id(), recovered.encryption_key_id());
    assert!(store.load(&scope, Slot::Candidate).await.unwrap().is_none());
}

#[tokio::test]
async fn explicit_candidate_recovery_keeps_transport_uncertainty_and_stops_after_one_replacement() {
    for transport in [true, false] {
        let mut rng = StdRng::seed_from_u64(73);
        let candidate = PrivateKeys::generate(&mut rng);
        let current = PrivateKeys::generate(&mut rng);
        let registry = RecoveryRegistry {
            active: Mutex::new(HashMap::from([("other-machine".into(), "vk_foreign".into())])),
            revoked: Default::default(), requests: Mutex::default(), transport, reject_all: true,
        };
        let store = Store::default();
        let scope = KeyScope::installation("machine", "owner").unwrap();
        store.save(&scope, Slot::Current, current.storage_bytes()).await.unwrap();
        store.save(&scope, Slot::Candidate, candidate.storage_bytes()).await.unwrap();
        let lifecycle = InstallationKeys { store: &store, registry: &registry, scope: scope.clone(), uid: "owner" };
        let expected = if transport { Error::Transport } else { Error::Revoked };
        assert!(matches!(lifecycle.enroll(&mut rng).await, Err(error) if error == expected));
        assert_eq!(registry.requests.lock().unwrap().len(), if transport { 1 } else { 2 });
        assert_eq!(lifecycle.load().await.unwrap().unwrap().encryption_key_id(), current.encryption_key_id());
        let saved = store.load(&scope, Slot::Candidate).await.unwrap();
        if transport {
            assert_eq!(PrivateKeys::from_storage(&saved.unwrap()).unwrap().encryption_key_id(), candidate.encryption_key_id());
        } else { assert!(saved.is_none()); }
        assert_eq!(registry.active.lock().unwrap()["other-machine"], "vk_foreign");
    }
}
