use super::*;

#[tokio::test]
async fn active_installation_key_is_reused_without_enrolling_again() {
    let store = Store::default();
    let mut rng = StdRng::seed_from_u64(29);
    let key = PrivateKeys::generate(&mut rng);
    let scope = KeyScope::installation("machine", "owner").unwrap();
    store.save(&scope, Slot::Current, key.storage_bytes()).await.unwrap();
    struct Existing(String);
    #[async_trait]
    impl KeyRegistry for Existing {
        async fn active_key_ids(&self) -> Result<Vec<String>> { Ok(vec![self.0.clone()]) }
        async fn enroll(&self, _: &str, _: &str) -> Result<String> { panic!("key is already enrolled") }
        async fn revoke(&self, _: &str) -> Result<()> { panic!("no revocation") }
    }
    let registry = Existing(key.verifier_key_id("owner"));
    let lifecycle = InstallationKeys { store: &store, registry: &registry, scope, uid: "owner" };
    for _ in 0..2 {
        assert_eq!(lifecycle.enroll(&mut rng).await.unwrap().encryption_key_id(), key.encryption_key_id());
    }
}
