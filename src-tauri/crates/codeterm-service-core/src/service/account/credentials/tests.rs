use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Legacy {
    reads: AtomicUsize,
    deletes: AtomicUsize,
}
#[async_trait]
impl LegacySource for Legacy {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<Secret>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(Some(Secret::new("legacy")))
    }
    async fn delete(&self, _: &str) {
        self.deletes.fetch_add(1, Ordering::SeqCst);
    }
}
fn credentials(root: &std::path::Path) -> AccountCredentials {
    AccountCredentials {
        local: Arc::new(ProtectedCredentials::new(root)),
        root: root.to_path_buf(),
    }
}

#[test]
fn test_credentials_refuse_a_missing_fixture_root() {
    assert!(credential_root(None).is_err());
}

#[tokio::test]
async fn fixture_credentials_share_one_store_and_isolate_other_fixtures() {
    use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
    let first = ServiceFixture::new(MultiplexerState::empty());
    let second = ServiceFixture::new(MultiplexerState::empty());
    let first_store = first.service.account_credentials().await.unwrap();
    let same_store = first.service.account_credentials().await.unwrap();
    let second_store = second.service.account_credentials().await.unwrap();
    assert!(Arc::ptr_eq(&first_store, &same_store));
    assert!(!Arc::ptr_eq(&first_store, &second_store));
    CredentialStore::save(first_store.as_ref(), "local", &Secret::new("first"))
        .await
        .unwrap();
    let key = credential_name("local");
    let persisted = |root: &std::path::Path| {
        ProtectedCredentials::new(root)
            .load(&key)
            .unwrap()
            .is_some()
    };
    assert!(persisted(first.temp.path()));
    assert!(!persisted(second.temp.path()));
    CredentialStore::save(second_store.as_ref(), "local", &Secret::new("second"))
        .await
        .unwrap();
    CredentialStore::delete(first_store.as_ref(), "local")
        .await
        .unwrap();
    assert!(!persisted(first.temp.path()));
    assert!(persisted(second.temp.path()));
}
#[tokio::test]
async fn account_and_mesh_round_trip_without_legacy_access() {
    let root = tempfile::tempdir().unwrap();
    let credentials = credentials(root.path());
    let native = Arc::new(Legacy::default());
    let legacy = LegacyStores {
        store: Arc::new(tokio::sync::RwLock::new(Arc::new(ForbiddenStore))),
        native: native.clone(),
    };
    CredentialStore::save(&credentials, "provider", &Secret::new("refresh"))
        .await
        .unwrap();
    codeterm_account::account_mesh::InstallationKeyStore::save(&credentials, &Secret::new("mesh"))
        .await
        .unwrap();
    assert!(
        !credentials
            .migrate_named(&credential_name("provider"), &legacy)
            .await
    );
    assert!(
        !credentials
            .migrate_named(MESH_INSTALLATION_KEY, &legacy)
            .await
    );
    let reopened = super::tests::credentials(root.path());
    assert_eq!(
        CredentialStore::load(&reopened, "provider")
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "refresh"
    );
    assert_eq!(
        codeterm_account::account_mesh::InstallationKeyStore::load(&reopened)
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "mesh"
    );
    assert_eq!(native.reads.load(Ordering::SeqCst), 0);
    assert_eq!(native.deletes.load(Ordering::SeqCst), 0);
    CredentialStore::delete(&credentials, "provider")
        .await
        .unwrap();
    assert!(CredentialStore::load(&credentials, "provider")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn migration_moves_once_and_deletes_only_after_persistence() {
    let root = tempfile::tempdir().unwrap();
    let credentials = credentials(root.path());
    let legacy = Legacy::default();
    let name = credential_name("provider");
    assert!(credentials.migrate_named(&name, &legacy).await);
    assert_eq!(
        credentials
            .load_named(name.clone())
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "legacy"
    );
    assert!(
        !super::tests::credentials(root.path())
            .migrate_named(&name, &legacy)
            .await
    );
    assert_eq!(legacy.reads.load(Ordering::SeqCst), 1);
    assert_eq!(legacy.deletes.load(Ordering::SeqCst), 1);
}

struct Slow;
#[async_trait]
impl LegacySource for Slow {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<Secret>> {
        std::future::pending().await
    }
    async fn delete(&self, _: &str) {
        panic!("no persisted credential");
    }
}
#[tokio::test]
async fn pending_migration_does_not_hold_local_operations_or_restore_after_logout() {
    let root = tempfile::tempdir().unwrap();
    let credentials = Arc::new(credentials(root.path()));
    let name = credential_name("provider");
    let claim = name.clone();
    credentials
        .blocking(move |local| local.claim_migration(&claim))
        .await
        .unwrap();
    CredentialStore::save(credentials.as_ref(), "provider", &Secret::new("new"))
        .await
        .unwrap();
    CredentialStore::delete(credentials.as_ref(), "provider")
        .await
        .unwrap();
    assert!(!credentials
        .local
        .finish_migration(&name, &Secret::new("old"))
        .unwrap());
    assert!(CredentialStore::load(credentials.as_ref(), "provider")
        .await
        .unwrap()
        .is_none());
    assert!(!credentials.migrate_named(&name, &Slow).await);
}
struct ForbiddenStore;
#[async_trait]
impl crate::secrets::SecretStore for ForbiddenStore {
    fn id(&self) -> &'static str {
        "forbidden"
    }
    async fn status(&self) -> crate::secrets::StoreStatus {
        panic!("unexpected vault access")
    }
    async fn unlock(
        &self,
        _: crate::secrets::UnlockCredentials,
    ) -> Result<(), crate::secrets::SecretError> {
        panic!("unexpected vault access")
    }
    async fn lock(&self) -> Result<(), crate::secrets::SecretError> {
        panic!("unexpected vault access")
    }
    async fn logout(&self) -> Result<(), crate::secrets::SecretError> {
        panic!("unexpected vault access")
    }
    async fn save(
        &self,
        _: crate::secrets::SaveRequest,
    ) -> Result<crate::secrets::SecretRef, crate::secrets::SecretError> {
        panic!("unexpected vault access")
    }
    async fn get(
        &self,
        _: &str,
    ) -> Result<crate::secrets::SecretValue, crate::secrets::SecretError> {
        panic!("unexpected vault access")
    }
    async fn list(
        &self,
        _: crate::secrets::ListFilter,
    ) -> Result<Vec<crate::secrets::SecretRef>, crate::secrets::SecretError> {
        panic!("unexpected vault access")
    }
    async fn search(
        &self,
        _: &str,
    ) -> Result<Vec<crate::secrets::SecretRef>, crate::secrets::SecretError> {
        panic!("unexpected vault access")
    }
    async fn delete(&self, _: &str) -> Result<(), crate::secrets::SecretError> {
        panic!("unexpected vault access")
    }
}

#[tokio::test(start_paused = true)]
async fn native_migration_timeout_is_bounded() {
    let legacy = LegacyStores {
        store: Arc::new(tokio::sync::RwLock::new(Arc::new(
            codeterm_secrets::mock::MockStore::new(),
        ))),
        native: Arc::new(Slow),
    };
    let start = tokio::time::Instant::now();
    assert!(matches!(legacy.load("absent").await, Err(Error::Timeout)));
    assert_eq!(start.elapsed(), Duration::from_secs(2));
}

#[test]
fn legacy_read_combination_preserves_errors_until_a_token_is_found() {
    assert!(matches!(
        combine_legacy_reads(Err(Error::Credentials), Ok(None)),
        Err(Error::Credentials)
    ));
    assert!(matches!(
        combine_legacy_reads(Err(Error::Timeout), Ok(None)),
        Err(Error::Timeout)
    ));
    assert!(matches!(
        combine_legacy_reads(Ok(None), Err(Error::Credentials)),
        Err(Error::Credentials)
    ));
    assert!(combine_legacy_reads(Ok(None), Ok(None)).unwrap().is_none());
    for (native, fallback) in [
        (Err(Error::Credentials), Ok(Some(Secret::new("found")))),
        (Ok(None), Ok(Some(Secret::new("found")))),
        (Ok(Some(Secret::new("found"))), Err(Error::Credentials)),
    ] {
        assert_eq!(
            combine_legacy_reads(native, fallback)
                .unwrap()
                .unwrap()
                .expose(),
            "found"
        );
    }
}

struct SignalledLegacy(tokio::sync::Notify);
#[async_trait]
impl LegacySource for SignalledLegacy {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<Secret>> {
        self.0.notify_one();
        std::future::pending().await
    }
    async fn delete(&self, _: &str) {
        panic!("no persisted credential");
    }
}
#[tokio::test]
async fn slow_legacy_read_does_not_block_private_file_save_or_load() {
    let root = tempfile::tempdir().unwrap();
    let credentials = credentials(root.path());
    let legacy = SignalledLegacy(tokio::sync::Notify::new());
    let name = credential_name("provider");
    let migration = credentials.migrate_named(&name, &legacy);
    tokio::pin!(migration);
    tokio::select! {
        _ = &mut migration => panic!("legacy read must remain pending"),
        _ = legacy.0.notified() => {}
    }
    CredentialStore::save(&credentials, "provider", &Secret::new("new"))
        .await
        .unwrap();
    assert_eq!(
        CredentialStore::load(&credentials, "provider")
            .await
            .unwrap()
            .unwrap()
            .expose(),
        "new"
    );
}
