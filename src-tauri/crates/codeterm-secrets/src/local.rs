use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::factory::SecretBackendFactory;
use super::secret_file::{SecretFile, SecretFileError};
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
        "Plaintext JSON file in ~/.domios/.secrets with user-only permissions (not encrypted, not the OS keychain). Offline, no external service."
    }
    fn build(&self, _cfg: &serde_json::Value) -> Box<dyn SecretStore> {
        Box::new(LocalStore::detect_and_build())
    }
}

const KEYCHAIN_SERVICE: &str = "domios-secrets";

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
    /// Next unallocated item number; persisted so ids survive restarts and deletes.
    #[serde(default)]
    next_id: u64,
}

impl LocalIndex {
    fn alloc_id(&mut self) -> String {
        let highest = self
            .entries
            .values()
            .filter_map(|e| e.item_id.strip_prefix("local-"))
            .filter_map(|n| u64::from_str_radix(n, 16).ok())
            .max()
            .unwrap_or(0);
        let n = self.next_id.max(highest + 1).max(1);
        self.next_id = n + 1;
        format!("local-{:08x}", n)
    }
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
        codeterm_paths::data_dir()
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
    keychain: SecretFile,
    index: Box<dyn IndexStorage>,
    /// Set when the local file backend construction failed. CRUD calls
    /// short-circuit to `Backend { message: <reason> }` so the user sees a
    /// clear error in Settings instead of mysteriously empty results.
    unavailable_reason: Option<String>,
}

impl LocalStore {
    /// Production constructor: local file-backed secret store + real on-disk index.
    pub fn detect_and_build() -> Self {
        let keychain = SecretFile::new(KEYCHAIN_SERVICE);
        let index: Box<dyn IndexStorage> =
            Box::new(FileIndexStorage::new(FileIndexStorage::default_path()));
        let unavailable = if keychain.is_available() {
            None
        } else {
            Some(format!(
                "local secret file unavailable: {} — secrets cannot be persisted",
                keychain.availability_reason().unwrap_or("unknown reason")
            ))
        };
        Self {
            keychain,
            index,
            unavailable_reason: unavailable,
        }
    }

    /// Test / DI constructor.
    pub fn new(keychain: SecretFile, index: Box<dyn IndexStorage>) -> Self {
        Self {
            keychain,
            index,
            unavailable_reason: None,
        }
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

    fn map_keychain_err(e: SecretFileError) -> SecretError {
        match e {
            SecretFileError::NotFound => SecretError::NotFound,
            SecretFileError::Unavailable(_) => SecretError::Locked,
            SecretFileError::Busy => SecretError::Backend {
                message: "Secret store is busy".into(),
            },
            SecretFileError::Other(msg) => SecretError::Backend { message: msg },
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
    fn allows_account_credentials(&self) -> bool {
        false
    }

    fn id(&self) -> &'static str {
        "local"
    }

    async fn status(&self) -> StoreStatus {
        if let Some(reason) = &self.unavailable_reason {
            return StoreStatus::Unavailable {
                reason: reason.clone(),
            };
        }
        let endpoint = Some("file:~/.domios/.secrets".to_string());
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
        // Local backend has no separate unlock. Treat unlock as a file/index probe.
        self.require_available()?;
        let _ = self.index.load()?;
        Ok(())
    }

    async fn lock(&self) -> Result<(), SecretError> {
        // Nothing to seal. We could clear an in-memory cache here in the future.
        self.require_available()?;
        Ok(())
    }

    async fn logout(&self) -> Result<(), SecretError> {
        // Local store has no remote session. Logout is a no-op; the user may delete individual entries explicitly.
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

        let is_overwrite = index.entries.contains_key(&key);
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
                item_id: index.alloc_id(),
                name: req.name.clone(),
                folder: req.folder.clone(),
                organization: req.organization.clone(),
                collection: req.collection.clone(),
                last_modified: now,
            }
        };

        let notes = match (&req.notes, is_overwrite) {
            (None, true) => self
                .keychain
                .get(&Self::keychain_account(&entry.name))
                .ok()
                .and_then(|raw| serde_json::from_str::<LocalEntry>(&raw).ok())
                .and_then(|e| e.notes),
            (n, _) => n.clone(),
        };
        let blob = LocalEntry {
            value: std::mem::take(&mut value_to_persist),
            notes,
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
        // Best-effort: drop the keychain entry first. If it's already gone we still want the index to converge.
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
mod tests;
