//! In-memory `SecretStore` used by tests and dev. Not persisted across
//! daemon restarts. Also doubles as the Phase-1 placeholder when the
//! configured backend (e.g. `bitwarden`) hasn't shipped its adapter yet.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use parking_lot::Mutex;

use super::{
    validate_name, ListFilter, SaveRequest, SecretError, SecretRef, SecretStore, SecretString,
    SecretValue, StoreStatus, UnlockCredentials,
};

#[derive(Clone)]
struct MockEntry {
    item_id: String,
    name: String,
    value: String,
    notes: Option<String>,
    folder: Option<String>,
    organization: Option<String>,
    collection: Option<String>,
    last_modified: i64,
}

#[derive(Default)]
struct Inner {
    /// Keyed by lowercased name for case-insensitive lookup.
    by_name: HashMap<String, MockEntry>,
    /// Inverse — by stable id.
    by_id: HashMap<String, String>,
    locked: bool,
    logged_in: bool,
    user: Option<String>,
}

pub struct MockStore {
    inner: Mutex<Inner>,
    next_id: AtomicUsize,
    /// When set, every operation returns `StoreStatus::Unavailable` and
    /// every CRUD call errors with `Backend { message: <reason> }`. Used as
    /// a placeholder for backends that haven't been built yet.
    unavailable_reason: Option<String>,
}

impl MockStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                logged_in: true,
                ..Inner::default()
            }),
            next_id: AtomicUsize::new(1),
            unavailable_reason: None,
        }
    }

    pub fn new_unavailable(reason: impl Into<String>) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            next_id: AtomicUsize::new(1),
            unavailable_reason: Some(reason.into()),
        }
    }

    fn alloc_id(&self) -> String {
        let n = self.next_id.fetch_add(1, Ordering::SeqCst);
        format!("mock-{:08x}", n)
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

    fn require_unlocked(&self) -> Result<(), SecretError> {
        self.require_available()?;
        let g = self.inner.lock();
        if !g.logged_in {
            return Err(SecretError::LoggedOut);
        }
        if g.locked {
            return Err(SecretError::Locked);
        }
        Ok(())
    }
}

impl Default for MockStore {
    fn default() -> Self {
        Self::new()
    }
}

fn entry_to_ref(e: &MockEntry) -> SecretRef {
    SecretRef {
        item_id: e.item_id.clone(),
        name: e.name.clone(),
        folder: e.folder.clone(),
        organization: e.organization.clone(),
        collection: e.collection.clone(),
        last_modified: Some(e.last_modified),
    }
}

fn matches_query(e: &MockEntry, q: &str) -> bool {
    let lower = q.to_lowercase();
    e.name.to_lowercase().contains(&lower)
        || e.notes
            .as_deref()
            .map(|n| n.to_lowercase().contains(&lower))
            .unwrap_or(false)
        || e.folder
            .as_deref()
            .map(|f| f.to_lowercase().contains(&lower))
            .unwrap_or(false)
}

fn matches_filter(e: &MockEntry, f: &ListFilter) -> bool {
    if let Some(folder) = &f.folder {
        if e.folder.as_deref() != Some(folder.as_str()) {
            return false;
        }
    }
    if let Some(org) = &f.organization {
        if e.organization.as_deref() != Some(org.as_str()) {
            return false;
        }
    }
    if let Some(coll) = &f.collection {
        if e.collection.as_deref() != Some(coll.as_str()) {
            return false;
        }
    }
    true
}

#[async_trait]
impl SecretStore for MockStore {
    fn id(&self) -> &'static str {
        "mock"
    }

    async fn status(&self) -> StoreStatus {
        if let Some(reason) = &self.unavailable_reason {
            return StoreStatus::Unavailable {
                reason: reason.clone(),
            };
        }
        let g = self.inner.lock();
        if !g.logged_in {
            return StoreStatus::LoggedOut;
        }
        if g.locked {
            return StoreStatus::Locked;
        }
        StoreStatus::Unlocked {
            user: g.user.clone(),
            transient: true,
        }
    }

    async fn unlock(&self, creds: UnlockCredentials) -> Result<(), SecretError> {
        self.require_available()?;
        if creds.master_password.is_empty() {
            return Err(SecretError::BadRequest {
                message: "master password must not be empty".into(),
            });
        }
        let mut g = self.inner.lock();
        g.logged_in = true;
        g.locked = false;
        if let Some(email) = creds.email {
            g.user = Some(email);
        } else if g.user.is_none() {
            g.user = Some("mock-user".into());
        }
        Ok(())
    }

    async fn lock(&self) -> Result<(), SecretError> {
        self.require_available()?;
        let mut g = self.inner.lock();
        g.locked = true;
        Ok(())
    }

    async fn logout(&self) -> Result<(), SecretError> {
        self.require_available()?;
        let mut g = self.inner.lock();
        g.locked = true;
        g.logged_in = false;
        g.user = None;
        Ok(())
    }

    async fn save(&self, req: SaveRequest) -> Result<SecretRef, SecretError> {
        self.require_unlocked()?;
        validate_name(&req.name)?;
        if req.value.is_empty() {
            return Err(SecretError::BadRequest {
                message: "value must not be empty".into(),
            });
        }

        let key = req.name.to_lowercase();
        let mut g = self.inner.lock();
        let now = Self::now_ms();

        if let Some(existing) = g.by_name.get(&key) {
            if !req.overwrite {
                return Err(SecretError::AlreadyExists);
            }
            let mut updated = existing.clone();
            updated.value = req.value.expose_secret().to_string();
            updated.notes = req.notes;
            updated.folder = req.folder;
            updated.organization = req.organization;
            updated.collection = req.collection;
            updated.last_modified = now;
            let r = entry_to_ref(&updated);
            g.by_name.insert(key, updated);
            return Ok(r);
        }

        let item_id = self.alloc_id();
        let entry = MockEntry {
            item_id: item_id.clone(),
            name: req.name.clone(),
            value: req.value.expose_secret().to_string(),
            notes: req.notes,
            folder: req.folder,
            organization: req.organization,
            collection: req.collection,
            last_modified: now,
        };
        let r = entry_to_ref(&entry);
        g.by_id.insert(item_id, key.clone());
        g.by_name.insert(key, entry);
        Ok(r)
    }

    async fn get(&self, name_or_id: &str) -> Result<SecretValue, SecretError> {
        self.require_unlocked()?;
        let g = self.inner.lock();
        let key = if let Some(name) = g.by_id.get(name_or_id) {
            name.clone()
        } else {
            name_or_id.to_lowercase()
        };
        let entry = g.by_name.get(&key).ok_or(SecretError::NotFound)?;
        Ok(SecretValue {
            item_id: entry.item_id.clone(),
            name: entry.name.clone(),
            value: SecretString::from(entry.value.clone()),
            notes: entry.notes.clone(),
        })
    }

    async fn list(&self, filter: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
        self.require_unlocked()?;
        let g = self.inner.lock();
        let mut out: Vec<SecretRef> = g
            .by_name
            .values()
            .filter(|e| matches_filter(e, &filter))
            .map(entry_to_ref)
            .collect();
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        if let Some(limit) = filter.limit {
            out.truncate(limit);
        }
        Ok(out)
    }

    async fn search(&self, query: &str) -> Result<Vec<SecretRef>, SecretError> {
        self.require_unlocked()?;
        let g = self.inner.lock();
        let q = query.trim();
        if q.is_empty() {
            return Err(SecretError::BadRequest {
                message: "query must not be empty".into(),
            });
        }
        let mut out: Vec<SecretRef> = g
            .by_name
            .values()
            .filter(|e| matches_query(e, q))
            .map(entry_to_ref)
            .collect();
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(out)
    }

    async fn delete(&self, name_or_id: &str) -> Result<(), SecretError> {
        self.require_unlocked()?;
        let mut g = self.inner.lock();
        let key = if let Some(name) = g.by_id.get(name_or_id) {
            name.clone()
        } else {
            name_or_id.to_lowercase()
        };
        match g.by_name.remove(&key) {
            Some(e) => {
                g.by_id.remove(&e.item_id);
                Ok(())
            }
            None => Err(SecretError::NotFound),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    async fn round_trip_save_get_delete() {
        let store = MockStore::new();
        let r = store.save(req("openai-prod", "sk-abc")).await.unwrap();
        assert_eq!(r.name, "openai-prod");
        assert!(!r.item_id.is_empty());

        let v = store.get("openai-prod").await.unwrap();
        assert_eq!(v.value.expose_secret(), "sk-abc");
        assert_eq!(v.name, "openai-prod");

        // case-insensitive name lookup
        let v2 = store.get("OPENAI-PROD").await.unwrap();
        assert_eq!(v2.value.expose_secret(), "sk-abc");

        // by id
        let v3 = store.get(&r.item_id).await.unwrap();
        assert_eq!(v3.value.expose_secret(), "sk-abc");

        store.delete("openai-prod").await.unwrap();
        assert!(matches!(
            store.get("openai-prod").await,
            Err(SecretError::NotFound)
        ));
    }

    #[tokio::test]
    async fn save_no_overwrite_errors_when_exists() {
        let store = MockStore::new();
        store.save(req("k", "v1")).await.unwrap();
        let err = store.save(req("k", "v2")).await.unwrap_err();
        assert!(matches!(err, SecretError::AlreadyExists));
    }

    #[tokio::test]
    async fn save_with_overwrite_replaces_value() {
        let store = MockStore::new();
        store.save(req("k", "v1")).await.unwrap();
        let mut r = req("k", "v2");
        r.overwrite = true;
        store.save(r).await.unwrap();
        let v = store.get("k").await.unwrap();
        assert_eq!(v.value.expose_secret(), "v2");
    }

    #[tokio::test]
    async fn list_and_search() {
        let store = MockStore::new();
        store.save(req("openai-prod", "v1")).await.unwrap();
        store.save(req("openai-staging", "v2")).await.unwrap();
        store.save(req("slack-webhook", "v3")).await.unwrap();

        let all = store.list(ListFilter::default()).await.unwrap();
        assert_eq!(all.len(), 3);
        // sorted alphabetically
        assert_eq!(all[0].name, "openai-prod");
        assert_eq!(all[1].name, "openai-staging");
        assert_eq!(all[2].name, "slack-webhook");

        let openai = store.search("openai").await.unwrap();
        assert_eq!(openai.len(), 2);

        let none = store.search("nonexistent").await.unwrap();
        assert!(none.is_empty());

        let with_limit = store
            .list(ListFilter {
                limit: Some(2),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(with_limit.len(), 2);
    }

    #[tokio::test]
    async fn list_with_folder_filter() {
        let store = MockStore::new();
        let mut r1 = req("a", "1");
        r1.folder = Some("work".into());
        store.save(r1).await.unwrap();
        let mut r2 = req("b", "2");
        r2.folder = Some("home".into());
        store.save(r2).await.unwrap();

        let work = store
            .list(ListFilter {
                folder: Some("work".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(work.len(), 1);
        assert_eq!(work[0].name, "a");
    }

    #[tokio::test]
    async fn locked_blocks_crud() {
        let store = MockStore::new();
        store.save(req("k", "v")).await.unwrap();
        store.lock().await.unwrap();
        assert!(matches!(store.get("k").await, Err(SecretError::Locked)));
        assert!(matches!(
            store.save(req("k2", "v")).await,
            Err(SecretError::Locked)
        ));
        assert!(matches!(
            store.list(ListFilter::default()).await,
            Err(SecretError::Locked)
        ));

        store
            .unlock(UnlockCredentials::with_password("pw".into()))
            .await
            .unwrap();
        let v = store.get("k").await.unwrap();
        assert_eq!(v.value.expose_secret(), "v");
    }

    #[tokio::test]
    async fn logged_out_after_logout() {
        let store = MockStore::new();
        store.save(req("k", "v")).await.unwrap();
        store.logout().await.unwrap();
        assert!(matches!(store.get("k").await, Err(SecretError::LoggedOut)));
        assert!(matches!(store.status().await, StoreStatus::LoggedOut));
    }

    #[tokio::test]
    async fn unavailable_store_blocks_everything() {
        let store = MockStore::new_unavailable("backend not built yet");
        match store.status().await {
            StoreStatus::Unavailable { reason } => {
                assert!(reason.contains("not built"));
            }
            other => panic!("expected Unavailable, got {:?}", other),
        }
        assert!(matches!(
            store.get("k").await,
            Err(SecretError::Backend { .. })
        ));
        assert!(matches!(
            store.save(req("k", "v")).await,
            Err(SecretError::Backend { .. })
        ));
    }

    #[tokio::test]
    async fn save_validates_inputs() {
        let store = MockStore::new();
        let mut bad_name = req("", "v");
        bad_name.value = SecretString::from("v");
        assert!(matches!(
            store.save(bad_name).await,
            Err(SecretError::BadRequest { .. })
        ));

        let bad_value = req("k", "");
        assert!(matches!(
            store.save(bad_value).await,
            Err(SecretError::BadRequest { .. })
        ));
    }

    #[tokio::test]
    async fn search_rejects_empty_query() {
        let store = MockStore::new();
        assert!(matches!(
            store.search("   ").await,
            Err(SecretError::BadRequest { .. })
        ));
    }

    #[tokio::test]
    async fn unlock_rejects_empty_password() {
        let store = MockStore::new();
        let creds = UnlockCredentials::with_password(SecretString::from(""));
        assert!(matches!(
            store.unlock(creds).await,
            Err(SecretError::BadRequest { .. })
        ));
    }
}
