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
    let mut old: AccountMeshRecord = serde_json::from_slice(&bytes).unwrap();
    assert!(old.peer_presence.is_empty());
    old.peer_presence = record.peer_presence.clone();
    assert!(value.get("peer_presence").is_none());
    assert!(path.with_file_name("mesh-peer-presence.json").exists());
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

#[tokio::test]
async fn candidate_inline_admission_cache_is_accepted_then_removed_on_save() {
    use super::super::{tests::support::Harness, EnrollTrigger};
    let directory = std::env::temp_dir().join(format!(
        "codeterm-mesh-inline-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let harness = Harness::new().await;
    harness.sign_in("candidate-owner").await;
    harness
        .mesh
        .refresh(EnrollTrigger::Automatic)
        .await
        .unwrap();
    let record = harness.store.record.lock().unwrap().clone();
    let mut candidate = serde_json::to_value(&record).unwrap();
    candidate["admission_cache"] =
        serde_json::to_value(record.admission_cache.as_ref().unwrap()).unwrap();
    let store = FileAccountMeshStore::new(directory.clone());
    let path = directory.join("state/account/mesh.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_vec(&candidate).unwrap()).unwrap();
    let accepted: AccountMeshRecord = serde_json::from_value(candidate).unwrap();
    assert!(accepted.admission_cache.is_some());
    let loaded = store.load().await.unwrap();
    assert_eq!(loaded.owner, record.owner);
    assert_eq!(loaded.keys, record.keys);
    assert_eq!(loaded.slots, record.slots);
    assert!(!loaded.signed_out);
    store.save(&loaded).await.unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(saved.get("admission_cache").is_none());
    assert_eq!(store.load().await.unwrap(), loaded);
    fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn corrupt_peer_presence_requires_refresh_without_poisoning_the_main_store() {
    let directory = tempfile::tempdir().unwrap();
    let store = FileAccountMeshStore::new(directory.path().into());
    let record = AccountMeshRecord {
        owner: Some("owner".into()),
        ..Default::default()
    };
    store.save(&record).await.unwrap();
    let path = directory
        .path()
        .join("state/account/mesh-peer-presence.json");
    fs::write(&path, b"corrupt-presence").unwrap();
    let mut loaded = store.load().await.unwrap();
    assert!(loaded.peer_presence_refresh);
    assert!(loaded.peer_presence.is_empty());
    store.save(&loaded).await.unwrap();
    assert!(store.load().await.unwrap().peer_presence_refresh);
    loaded.peer_presence_refresh = false;
    store.save(&loaded).await.unwrap();
    assert!(!store.load().await.unwrap().peer_presence_refresh);
    assert_eq!(fs::read(path).unwrap(), b"{}");
}
