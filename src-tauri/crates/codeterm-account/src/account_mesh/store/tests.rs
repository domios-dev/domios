use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
async fn unreadable_mesh_file_is_preserved_and_cannot_be_overwritten() {
    let directory = std::env::temp_dir().join(format!(
        "codeterm-mesh-store-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("mesh.json");
    let corrupt = b"{not-json";
    fs::write(&path, corrupt).unwrap();
    let store = FileAccountMeshStore { path: path.clone() };

    assert!(store.load().await.is_err());
    assert!(store.save(&AccountMeshRecord::default()).await.is_err());
    assert_eq!(fs::read(&path).unwrap(), corrupt);

    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn admission_cache_is_separate_and_mesh_json_remains_rollback_readable() {
    use super::super::tests::support::Harness;
    use super::super::{AccountMeshRecord, EnrollTrigger};
    let directory = std::env::temp_dir().join(format!(
        "codeterm-mesh-rollback-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let harness = Harness::new().await;
    harness.sign_in("rollback-owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let record = harness.store.record.lock().unwrap().clone();
    assert!(record.admission_cache.is_some());
    let store = FileAccountMeshStore::new(directory.clone());
    store.save(&record).await.unwrap();
    let path = directory.join("state/account/mesh.json");
    let bytes = fs::read(&path).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value.get("admission_cache").is_none());
    #[derive(serde::Deserialize, serde::Serialize)]
    #[serde(deny_unknown_fields)]
    struct RollbackRecord {
        owner: Option<String>,
        signed_out: bool,
        cleanup_pending: bool,
        published: bool,
        keys: std::collections::BTreeMap<String, super::super::state::KeyBinding>,
        slots: std::collections::BTreeMap<String, u64>,
        revoked_installations: std::collections::BTreeSet<String>,
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        revoked_generations: std::collections::BTreeMap<String, u64>,
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        peer_keys: std::collections::BTreeMap<String, super::super::state::KeyBinding>,
    }
    let rollback: RollbackRecord = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_value(rollback).unwrap(), value);
    let old: AccountMeshRecord = serde_json::from_slice(&bytes).unwrap();
    assert!(old.admission_cache.is_none());
    assert_eq!(old.keys, record.keys);
    assert_eq!(old.slots, record.slots);
    assert_eq!(old.owner, record.owner);
    assert_eq!(store.load().await.unwrap(), record);
    let cache_path = path.with_file_name("mesh-admission.json");
    assert!(cache_path.exists());
    fs::write(&cache_path, b"bad-cache").unwrap();
    let without_cache = store.load().await.unwrap();
    assert_eq!(without_cache, old);
    store.save(&record).await.unwrap();
    store.save(&old).await.unwrap();
    assert!(!cache_path.exists());
    fs::remove_dir_all(directory).unwrap();
}
