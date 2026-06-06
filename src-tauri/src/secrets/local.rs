use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::factory::SecretBackendFactory;
use super::keychain::{Keychain, KeychainError};
use super::{
    validate_name, ListFilter, SaveRequest, SecretError, SecretRef, SecretStore, SecretString,
    SecretValue, StoreStatus, UnlockCredentials,
};

pub struct LocalFactory;

impl SecretBackendFactory for LocalFactory {
    fn id(&self) -> &'static str {
        "local"
    }
    fn label(&self) -> &'static str {
        "Local secret storage"
    }
    fn description(&self) -> &'static str {
        "Persisted in the OS keychain (macOS Keychain / libsecret / Windows Credential Manager). Offline, no external service."
    }
    fn build(&self, _cfg: &serde_json::Value) -> Box<dyn SecretStore> {
        Box::new(LocalStore::detect_and_build())
    }
}

const KEYCHAIN_SERVICE: &str = "codeterm-secrets";

/// What lives behind a single keychain entry. Stored JSON-encoded.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalEntry {
    value: String,
    #[serde(default)]
    notes: Option<String>,
}

/// Per-secret metadata stored in the on-disk index. NEVER contains the
/// secret value — only what's safe to read without an unlocked keychain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    item_id: String,
    name: String,
    #[serde(default)]
    folder: Option<String>,
    #[serde(default)]
    organization: Option<String>,
    #[serde(default)]
    collection: Option<String>,
    last_modified: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalIndex {
    /// Schema version — bump when the layout changes.
    #[serde(default = "default_index_version")]
    version: u32,
    /// Keyed by lowercased name for case-insensitive lookup.
    entries: HashMap<String, IndexEntry>,
}

fn default_index_version() -> u32 {
    1
}

impl IndexEntry {
    fn into_ref(&self) -> SecretRef {
        SecretRef {
            item_id: self.item_id.clone(),
            name: self.name.clone(),
            folder: self.folder.clone(),
            organization: self.organization.clone(),
            collection: self.collection.clone(),
            last_modified: Some(self.last_modified),
        }
    }
}

/// Storage interface for the index file. Lets tests inject an in-memory
/// fake without touching the real filesystem.
pub trait IndexStorage: Send + Sync {
    fn load(&self) -> Result<LocalIndex, SecretError>;
    fn save(&self, index: &LocalIndex) -> Result<(), SecretError>;
}

/// Real on-disk index. Uses an atomic write (`tmp -> rename`) to survive
/// crashes mid-update.
pub struct FileIndexStorage {
    path: PathBuf,
}

impl FileIndexStorage {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> PathBuf {
        crate::paths::data_dir()
            .join("secrets")
            .join("local-index.json")
    }
}

impl IndexStorage for FileIndexStorage {
    fn load(&self) -> Result<LocalIndex, SecretError> {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => serde_json::from_str(&s).map_err(|e| SecretError::Backend {
                message: format!("parse index: {}", e),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(LocalIndex::default()),
            Err(e) => Err(SecretError::Backend {
                message: format!("read index: {}", e),
            }),
        }
    }

    fn save(&self, index: &LocalIndex) -> Result<(), SecretError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| SecretError::Backend {
                message: format!("create index dir: {}", e),
            })?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(index).map_err(|e| SecretError::Backend {
            message: format!("encode index: {}", e),
        })?;
        std::fs::write(&tmp, body).map_err(|e| SecretError::Backend {
            message: format!("write index tmp: {}", e),
        })?;
        std::fs::rename(&tmp, &self.path).map_err(|e| SecretError::Backend {
            message: format!("rename index: {}", e),
        })?;
        Ok(())
    }
}

/// In-memory storage used by tests. Keeps a single index in a Mutex.
pub struct MemIndexStorage {
    inner: Mutex<LocalIndex>,
}

impl MemIndexStorage {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(LocalIndex::default()),
        }
    }
}

impl IndexStorage for MemIndexStorage {
    fn load(&self) -> Result<LocalIndex, SecretError> {
        Ok(self.inner.lock().clone())
    }

    fn save(&self, index: &LocalIndex) -> Result<(), SecretError> {
        *self.inner.lock() = index.clone();
        Ok(())
    }
}

pub struct LocalStore {
    keychain: Keychain,
    index: Box<dyn IndexStorage>,
    next_id: AtomicUsize,
    /// Set when the keychain probe at construction time failed. CRUD calls
    /// short-circuit to `Backend { message: <reason> }` so the user sees a
    /// clear error in Settings instead of mysteriously empty results.
    unavailable_reason: Option<String>,
}

impl LocalStore {
    /// Production constructor: real OS keychain + real on-disk index.
    pub fn detect_and_build() -> Self {
        let keychain = Keychain::new(KEYCHAIN_SERVICE);
        let index: Box<dyn IndexStorage> =
            Box::new(FileIndexStorage::new(FileIndexStorage::default_path()));
        let unavailable = if keychain.is_available() {
            None
        } else {
            Some(format!(
                "OS keychain unavailable: {} — secrets cannot be persisted",
                keychain.availability_reason().unwrap_or("unknown reason")
            ))
        };
        Self {
            keychain,
            index,
            next_id: AtomicUsize::new(1),
            unavailable_reason: unavailable,
        }
    }

    /// Test / DI constructor.
    pub fn new(keychain: Keychain, index: Box<dyn IndexStorage>) -> Self {
        Self {
            keychain,
            index,
            next_id: AtomicUsize::new(1),
            unavailable_reason: None,
        }
    }

    fn alloc_id(&self) -> String {
        let n = self.next_id.fetch_add(1, Ordering::SeqCst);
        format!("local-{:08x}", n)
    }

    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    fn require_available(&self) -> Result<(), SecretError> {
        if let Some(reason) = &self.unavailable_reason {
            return Err(SecretError::Backend {
                message: reason.clone(),
            });
        }
        Ok(())
    }

    fn keychain_account(name: &str) -> String {
        name.to_lowercase()
    }

    fn map_keychain_err(e: KeychainError) -> SecretError {
        match e {
            KeychainError::NotFound => SecretError::NotFound,
            KeychainError::Unavailable(_) => SecretError::Locked,
            KeychainError::Other(msg) => SecretError::Backend { message: msg },
        }
    }

    fn lookup_by_id<'a>(&self, index: &'a LocalIndex, id_or_name: &str) -> Option<&'a IndexEntry> {
        let key = id_or_name.to_lowercase();
        if let Some(direct) = index.entries.get(&key) {
            return Some(direct);
        }
        index.entries.values().find(|e| e.item_id == id_or_name)
    }
}

#[async_trait]
impl SecretStore for LocalStore {
    fn id(&self) -> &'static str {
        "local"
    }

    async fn status(&self) -> StoreStatus {
        if let Some(reason) = &self.unavailable_reason {
            return StoreStatus::Unavailable {
                reason: reason.clone(),
            };
        }
        // Probe by attempting to load the index and reach the OS keychain.
        let endpoint = Some(format!("os-keychain:{}", KEYCHAIN_SERVICE));
        match self.index.load() {
            Ok(_) => StoreStatus::Unlocked {
                user: hostname_label(),
                transient: !self.keychain.is_available(),
                endpoint,
            },
            Err(_) => StoreStatus::Locked { endpoint },
        }
    }

    async fn unlock(&self, _creds: UnlockCredentials) -> Result<(), SecretError> {
        // Local backend has no separate unlock — the OS keychain is unlocked
        // by the user's login session. We treat unlock as a probe: if the
        // index loads and the keychain is reachable, we're good.
        self.require_available()?;
        let _ = self.index.load()?;
        Ok(())
    }

    async fn lock(&self) -> Result<(), SecretError> {
        // Nothing to seal — entries are gated by the OS keychain itself.
        // We could clear an in-memory cache here in the future.
        self.require_available()?;
        Ok(())
    }

    async fn logout(&self) -> Result<(), SecretError> {
        // Local store has no remote session. Logout is a no-op; the user
        // may delete individual entries explicitly.
        self.require_available()?;
        Ok(())
    }

    async fn save(&self, req: SaveRequest) -> Result<SecretRef, SecretError> {
        self.require_available()?;
        validate_name(&req.name)?;
        if req.value.is_empty() {
            return Err(SecretError::BadRequest {
                message: "value must not be empty".into(),
            });
        }

        let mut index = self.index.load()?;
        let key = req.name.to_lowercase();
        let now = Self::now_ms();
        let mut value_to_persist = req.value.expose_secret().to_string();

        let entry = if let Some(existing) = index.entries.get(&key).cloned() {
            if !req.overwrite {
                return Err(SecretError::AlreadyExists);
            }
            IndexEntry {
                item_id: existing.item_id,
                name: req.name.clone(),
                folder: req.folder.clone(),
                organization: req.organization.clone(),
                collection: req.collection.clone(),
                last_modified: now,
            }
        } else {
            IndexEntry {
                item_id: self.alloc_id(),
                name: req.name.clone(),
                folder: req.folder.clone(),
                organization: req.organization.clone(),
                collection: req.collection.clone(),
                last_modified: now,
            }
        };

        let blob = LocalEntry {
            value: std::mem::take(&mut value_to_persist),
            notes: req.notes.clone(),
        };
        let json = serde_json::to_string(&blob).map_err(|e| SecretError::Backend {
            message: format!("encode entry: {}", e),
        })?;

        self.keychain
            .set(&Self::keychain_account(&entry.name), &json)
            .map_err(Self::map_keychain_err)?;

        let r = entry.into_ref();
        index.entries.insert(key, entry);
        self.index.save(&index)?;
        Ok(r)
    }

    async fn get(&self, name_or_id: &str) -> Result<SecretValue, SecretError> {
        self.require_available()?;
        let index = self.index.load()?;
        let entry = self
            .lookup_by_id(&index, name_or_id)
            .ok_or(SecretError::NotFound)?
            .clone();
        let raw = self
            .keychain
            .get(&Self::keychain_account(&entry.name))
            .map_err(Self::map_keychain_err)?;
        let blob: LocalEntry = serde_json::from_str(&raw).map_err(|e| SecretError::Backend {
            message: format!("decode entry: {}", e),
        })?;
        Ok(SecretValue {
            item_id: entry.item_id,
            name: entry.name,
            value: SecretString::from(blob.value),
            notes: blob.notes,
        })
    }

    async fn list(&self, filter: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
        self.require_available()?;
        let index = self.index.load()?;
        let mut out: Vec<SecretRef> = index
            .entries
            .values()
            .filter(|e| matches_filter(e, &filter))
            .map(IndexEntry::into_ref)
            .collect();
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        if let Some(limit) = filter.limit {
            out.truncate(limit);
        }
        Ok(out)
    }

    async fn search(&self, query: &str) -> Result<Vec<SecretRef>, SecretError> {
        self.require_available()?;
        let q = query.trim();
        if q.is_empty() {
            return Err(SecretError::BadRequest {
                message: "query must not be empty".into(),
            });
        }
        let lower = q.to_lowercase();
        let index = self.index.load()?;
        let mut out: Vec<SecretRef> = index
            .entries
            .values()
            .filter(|e| {
                e.name.to_lowercase().contains(&lower)
                    || e.folder
                        .as_deref()
                        .map(|f| f.to_lowercase().contains(&lower))
                        .unwrap_or(false)
            })
            .map(IndexEntry::into_ref)
            .collect();
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(out)
    }

    async fn delete(&self, name_or_id: &str) -> Result<(), SecretError> {
        self.require_available()?;
        let mut index = self.index.load()?;
        let entry = self
            .lookup_by_id(&index, name_or_id)
            .ok_or(SecretError::NotFound)?
            .clone();
        let key = entry.name.to_lowercase();
        // Best-effort: drop the keychain entry first. If it's already gone
        // we still want the index to converge.
        let _ = self.keychain.delete(&Self::keychain_account(&entry.name));
        index.entries.remove(&key);
        self.index.save(&index)?;
        Ok(())
    }
}

fn matches_filter(entry: &IndexEntry, f: &ListFilter) -> bool {
    if let Some(folder) = &f.folder {
        if entry.folder.as_deref() != Some(folder.as_str()) {
            return false;
        }
    }
    if let Some(org) = &f.organization {
        if entry.organization.as_deref() != Some(org.as_str()) {
            return false;
        }
    }
    if let Some(coll) = &f.collection {
        if entry.collection.as_deref() != Some(coll.as_str()) {
            return false;
        }
    }
    true
}

fn hostname_label() -> Option<String> {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| hostname::get().ok().and_then(|os| os.into_string().ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> LocalStore {
        LocalStore::new(
            Keychain::in_memory("test"),
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

        // Case-insensitive lookup
        let v2 = s.get("OPENAI-PROD").await.unwrap();
        assert_eq!(v2.value.expose_secret(), "sk-abc");

        // Lookup by id
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
        // Idempotency: deleting twice should not panic, but a second
        // delete on a missing entry is NotFound.
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
        // Capture what the store actually writes to its index storage
        // and assert the secret value is nowhere in it.
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
        let s = LocalStore::new(Keychain::in_memory("test"), Box::new(storage));
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
}
