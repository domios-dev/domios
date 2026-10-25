use super::*;

#[test]
fn secret_string_does_not_leak_in_debug() {
    let s = SecretString::from("super-secret");
    let debug = format!("{:?}", s);
    assert!(!debug.contains("super-secret"));
    assert!(debug.contains("len=12"));

    let display = format!("{}", s);
    assert!(!display.contains("super-secret"));
    assert!(display.contains("REDACTED"));
}

#[test]
fn validate_name_accepts_normal_input() {
    assert!(validate_name("openai-prod").is_ok());
    assert!(validate_name("foo.bar/baz").is_ok());
}

#[test]
fn validate_name_rejects_pathological_input() {
    assert!(matches!(
        validate_name(""),
        Err(SecretError::BadRequest { .. })
    ));
    assert!(matches!(
        validate_name(" leading"),
        Err(SecretError::BadRequest { .. })
    ));
    assert!(matches!(
        validate_name("trailing "),
        Err(SecretError::BadRequest { .. })
    ));
    assert!(matches!(
        validate_name("contains\0nul"),
        Err(SecretError::BadRequest { .. })
    ));
    let oversized = "a".repeat(129);
    assert!(matches!(
        validate_name(&oversized),
        Err(SecretError::BadRequest { .. })
    ));
}

fn cfg_with(backend: &str) -> codeterm_config::schema::SecretsConfig {
    codeterm_config::schema::SecretsConfig {
        backend: backend.to_string(),
        ..Default::default()
    }
}

fn empty_registry() -> std::sync::Arc<SecretRegistry> {
    std::sync::Arc::new(SecretRegistry::new())
}

#[test]
fn build_active_store_falls_back_to_local_for_unknown_backend() {
    assert_eq!(
        build_active_store(&cfg_with("garbage"), &empty_registry()).id(),
        "local"
    );
}

#[test]
fn build_active_store_local_is_default() {
    assert_eq!(
        build_active_store(&cfg_with("local"), &empty_registry()).id(),
        "local"
    );
}

#[test]
fn build_active_store_mock_still_available_for_tests() {
    assert_eq!(
        build_active_store(&cfg_with("mock"), &empty_registry()).id(),
        "mock"
    );
}

#[test]
fn build_active_store_prefers_registered_plugin_instance() {
    let reg = empty_registry();
    reg.register(
        "some-plugin-store",
        std::sync::Arc::new(mock::MockStore::default()),
    );
    // A registered plugin instance wins over the native factory fallback.
    let store = build_active_store(&cfg_with("some-plugin-store"), &reg);
    assert_eq!(store.id(), mock::MockStore::default().id());
}

struct GetOnlyStore {
    present: bool,
    get_calls: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl SecretStore for GetOnlyStore {
    fn id(&self) -> &'static str {
        "get-only-test"
    }
    async fn status(&self) -> StoreStatus {
        StoreStatus::Unlocked {
            user: None,
            transient: true,
            endpoint: None,
        }
    }
    async fn unlock(&self, _: UnlockCredentials) -> Result<(), SecretError> {
        Ok(())
    }
    async fn lock(&self) -> Result<(), SecretError> {
        Ok(())
    }
    async fn logout(&self) -> Result<(), SecretError> {
        Ok(())
    }
    async fn save(&self, _: SaveRequest) -> Result<SecretRef, SecretError> {
        Err(SecretError::BadRequest {
            message: "unsupported".into(),
        })
    }
    async fn get(&self, name_or_id: &str) -> Result<SecretValue, SecretError> {
        self.get_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.present {
            Ok(SecretValue {
                item_id: "id-1".into(),
                name: name_or_id.into(),
                value: SecretString::from("v"),
                notes: None,
            })
        } else {
            Err(SecretError::NotFound)
        }
    }
    async fn list(&self, _: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
        Ok(Vec::new())
    }
    async fn search(&self, _: &str) -> Result<Vec<SecretRef>, SecretError> {
        Ok(Vec::new())
    }
    async fn delete(&self, _: &str) -> Result<(), SecretError> {
        Ok(())
    }
}

#[tokio::test]
async fn default_has_impl_returns_true_for_existing_item() {
    let store = GetOnlyStore {
        present: true,
        get_calls: std::sync::atomic::AtomicUsize::new(0),
    };
    assert_eq!(store.has("anything").await.unwrap(), true);
    assert_eq!(store.get_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn default_has_impl_returns_false_for_not_found() {
    let store = GetOnlyStore {
        present: false,
        get_calls: std::sync::atomic::AtomicUsize::new(0),
    };
    assert_eq!(store.has("missing").await.unwrap(), false);
    assert_eq!(store.get_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn default_has_impl_propagates_other_errors() {
    struct LockedStore;
    #[async_trait]
    impl SecretStore for LockedStore {
        fn id(&self) -> &'static str {
            "locked-test"
        }
        async fn status(&self) -> StoreStatus {
            StoreStatus::Locked { endpoint: None }
        }
        async fn unlock(&self, _: UnlockCredentials) -> Result<(), SecretError> {
            Ok(())
        }
        async fn lock(&self) -> Result<(), SecretError> {
            Ok(())
        }
        async fn logout(&self) -> Result<(), SecretError> {
            Ok(())
        }
        async fn save(&self, _: SaveRequest) -> Result<SecretRef, SecretError> {
            Err(SecretError::Locked)
        }
        async fn get(&self, _: &str) -> Result<SecretValue, SecretError> {
            Err(SecretError::Locked)
        }
        async fn list(&self, _: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
            Err(SecretError::Locked)
        }
        async fn search(&self, _: &str) -> Result<Vec<SecretRef>, SecretError> {
            Err(SecretError::Locked)
        }
        async fn delete(&self, _: &str) -> Result<(), SecretError> {
            Err(SecretError::Locked)
        }
    }
    let store = LockedStore;
    assert!(matches!(store.has("k").await, Err(SecretError::Locked)));
}
