use super::*;

fn store() -> LocalStore {
    LocalStore::new(
        SecretFile::in_memory("test"),
        Box::new(MemIndexStorage::new()),
    )
}

fn req(name: &str, value: &str) -> SaveRequest {
    SaveRequest {
        name: name.into(),
        value: SecretString::from(value),
        notes: None,
        folder: None,
        organization: None,
        collection: None,
        overwrite: false,
    }
}

#[test]
fn plaintext_storage_cannot_receive_account_credentials() {
    assert!(!store().allows_account_credentials());
}

#[tokio::test]
async fn id_is_local() {
    let s = store();
    assert_eq!(s.id(), "local");
}

#[tokio::test]
async fn round_trip_save_get() {
    let s = store();
    let r = s.save(req("openai-prod", "sk-abc")).await.unwrap();
    assert_eq!(r.name, "openai-prod");
    assert!(r.item_id.starts_with("local-"));

    let v = s.get("openai-prod").await.unwrap();
    assert_eq!(v.value.expose_secret(), "sk-abc");

    let v2 = s.get("OPENAI-PROD").await.unwrap();
    assert_eq!(v2.value.expose_secret(), "sk-abc");

    let v3 = s.get(&r.item_id).await.unwrap();
    assert_eq!(v3.value.expose_secret(), "sk-abc");
}

#[tokio::test]
async fn save_no_overwrite_errors_when_exists() {
    let s = store();
    s.save(req("k", "v1")).await.unwrap();
    let err = s.save(req("k", "v2")).await.unwrap_err();
    assert!(matches!(err, SecretError::AlreadyExists));
}

#[tokio::test]
async fn save_with_overwrite_replaces_value_and_keeps_id() {
    let s = store();
    let r1 = s.save(req("k", "v1")).await.unwrap();
    let mut r2 = req("k", "v2");
    r2.overwrite = true;
    let r2_ref = s.save(r2).await.unwrap();
    // Same item id across overwrite — important for stable references.
    assert_eq!(r1.item_id, r2_ref.item_id);
    let v = s.get("k").await.unwrap();
    assert_eq!(v.value.expose_secret(), "v2");
}

#[tokio::test]
async fn delete_removes_index_and_keychain() {
    let s = store();
    s.save(req("k", "v")).await.unwrap();
    s.delete("k").await.unwrap();
    assert!(matches!(s.get("k").await, Err(SecretError::NotFound)));
    // Idempotency: deleting twice should not panic, but a second delete on a missing entry is NotFound.
    assert!(matches!(s.delete("k").await, Err(SecretError::NotFound)));
}

#[tokio::test]
async fn list_sorts_alphabetically_and_filters_by_folder() {
    let s = store();
    s.save(req("zeta", "v")).await.unwrap();
    s.save(req("alpha", "v")).await.unwrap();
    let mut work = req("work-secret", "v");
    work.folder = Some("work".into());
    s.save(work).await.unwrap();

    let all = s.list(ListFilter::default()).await.unwrap();
    let names: Vec<_> = all.iter().map(|r| r.name.clone()).collect();
    assert_eq!(names, vec!["alpha", "work-secret", "zeta"]);

    let only_work = s
        .list(ListFilter {
            folder: Some("work".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(only_work.len(), 1);
    assert_eq!(only_work[0].name, "work-secret");
}

#[tokio::test]
async fn search_matches_name_and_folder_substring() {
    let s = store();
    s.save(req("openai-prod", "v")).await.unwrap();
    s.save(req("slack-bot", "v")).await.unwrap();
    let mut grouped = req("misc", "v");
    grouped.folder = Some("openai".into());
    s.save(grouped).await.unwrap();

    let hits = s.search("openai").await.unwrap();
    let names: Vec<_> = hits.iter().map(|r| r.name.clone()).collect();
    assert!(names.contains(&"openai-prod".to_string()));
    assert!(names.contains(&"misc".to_string()));
    assert!(!names.contains(&"slack-bot".to_string()));
}

#[tokio::test]
async fn search_rejects_empty_query() {
    let s = store();
    assert!(matches!(
        s.search("   ").await,
        Err(SecretError::BadRequest { .. })
    ));
}

#[tokio::test]
async fn save_validates_name_and_value() {
    let s = store();
    let bad_name = req("", "v");
    assert!(matches!(
        s.save(bad_name).await,
        Err(SecretError::BadRequest { .. })
    ));

    let bad_value = req("k", "");
    assert!(matches!(
        s.save(bad_value).await,
        Err(SecretError::BadRequest { .. })
    ));
}

#[tokio::test]
async fn status_unlocked_when_index_loads() {
    let s = store();
    match s.status().await {
        StoreStatus::Unlocked { transient, .. } => {
            // In-memory keychain marks transient=true.
            assert!(transient);
        }
        other => panic!("expected Unlocked, got {:?}", other),
    }
}

#[tokio::test]
async fn index_never_serializes_secret_material() {
    // Capture what the store writes to its index storage and assert the secret value is nowhere in it.
    let captured: std::sync::Arc<parking_lot::Mutex<Vec<String>>> =
        std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));

    struct CapturingIndex {
        inner: Mutex<LocalIndex>,
        captured: std::sync::Arc<parking_lot::Mutex<Vec<String>>>,
    }
    impl IndexStorage for CapturingIndex {
        fn load(&self) -> Result<LocalIndex, SecretError> {
            Ok(self.inner.lock().clone())
        }
        fn save(&self, index: &LocalIndex) -> Result<(), SecretError> {
            let json = serde_json::to_string(index).unwrap();
            self.captured.lock().push(json);
            *self.inner.lock() = index.clone();
            Ok(())
        }
    }

    let storage = CapturingIndex {
        inner: Mutex::new(LocalIndex::default()),
        captured: captured.clone(),
    };
    let s = LocalStore::new(SecretFile::in_memory("test"), Box::new(storage));
    s.save(req("k", "super-secret-value")).await.unwrap();

    let writes = captured.lock();
    assert!(!writes.is_empty());
    for w in writes.iter() {
        assert!(
            !w.contains("super-secret-value"),
            "index leaked secret material: {}",
            w
        );
    }
}

#[tokio::test]
async fn unavailable_store_short_circuits() {
    let mut s = store();
    s.unavailable_reason = Some("test reason".into());
    assert!(matches!(s.status().await, StoreStatus::Unavailable { .. }));
    assert!(matches!(
        s.get("anything").await,
        Err(SecretError::Backend { .. })
    ));
    assert!(matches!(
        s.save(req("k", "v")).await,
        Err(SecretError::Backend { .. })
    ));
}

#[tokio::test]
async fn list_with_org_filter() {
    let s = store();
    let mut a = req("a", "v");
    a.organization = Some("org-1".into());
    s.save(a).await.unwrap();
    let mut b = req("b", "v");
    b.organization = Some("org-2".into());
    s.save(b).await.unwrap();

    let only = s
        .list(ListFilter {
            organization: Some("org-1".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(only.len(), 1);
    assert_eq!(only[0].name, "a");
}

#[tokio::test]
async fn overwrite_without_notes_keeps_existing_notes() {
    let s = store();
    let mut first = req("k", "v1");
    first.notes = Some("keep me".into());
    s.save(first).await.unwrap();
    let mut second = req("k", "v2");
    second.overwrite = true;
    s.save(second).await.unwrap();
    assert_eq!(s.get("k").await.unwrap().notes.as_deref(), Some("keep me"));
}

#[tokio::test]
async fn secret_ids_persist_across_restart() {
    use std::sync::Arc;
    struct Shared(Arc<Mutex<LocalIndex>>);
    impl IndexStorage for Shared {
        fn load(&self) -> Result<LocalIndex, SecretError> {
            Ok(self.0.lock().clone())
        }
        fn save(&self, index: &LocalIndex) -> Result<(), SecretError> {
            *self.0.lock() = index.clone();
            Ok(())
        }
    }
    let disk = Arc::new(Mutex::new(LocalIndex::default()));
    let file = SecretFile::in_memory("test");
    let first = LocalStore::new(file.clone(), Box::new(Shared(disk.clone())));
    let a = first.save(req("a", "v")).await.unwrap();
    let b = first.save(req("b", "v")).await.unwrap();
    drop(first);
    let second = LocalStore::new(file, Box::new(Shared(disk)));
    let c = second.save(req("c", "v")).await.unwrap();
    second.delete("c").await.unwrap();
    let d = second.save(req("d", "v")).await.unwrap();
    let ids = [a.item_id, b.item_id, c.item_id, d.item_id];
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
}

#[tokio::test]
async fn list_respects_limit() {
    let s = store();
    for i in 0..5 {
        s.save(req(&format!("k{}", i), "v")).await.unwrap();
    }
    let limited = s
        .list(ListFilter {
            limit: Some(2),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(limited.len(), 2);
}

#[test]
fn a_contended_local_file_is_reported_as_busy_without_requesting_vault_unlock() {
    let error = LocalStore::map_keychain_err(SecretFileError::Busy);
    assert!(
        matches!(&error, SecretError::Backend { message } if message == "Secret store is busy")
    );
}
