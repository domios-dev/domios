use std::collections::HashMap;
use std::sync::OnceLock;

use parking_lot::{
    MappedRwLockReadGuard, MappedRwLockWriteGuard, RwLock, RwLockReadGuard, RwLockWriteGuard,
};

pub(crate) struct ScopedCache<T>(OnceLock<RwLock<HashMap<String, T>>>);

impl<T: Default> ScopedCache<T> {
    pub(crate) const fn new() -> Self {
        Self(OnceLock::new())
    }

    fn key() -> String {
        super::current()
            .map(|context| {
                format!(
                    "{}:{}",
                    context.cache_key(),
                    crate::model_catalog::seed_fingerprint(context.provider())
                )
            })
            .unwrap_or_default()
    }

    fn entries(&self) -> &RwLock<HashMap<String, T>> {
        self.0.get_or_init(RwLock::default)
    }

    fn admit(entries: &mut HashMap<String, T>, key: &str) {
        if !entries.contains_key(key) && entries.len() >= 128 {
            if let Some(old) = entries.keys().find(|key| !key.is_empty()).cloned() {
                entries.remove(&old);
            }
        }
        entries.entry(key.to_owned()).or_default();
    }

    pub(crate) fn read(&self) -> MappedRwLockReadGuard<'_, T> {
        let key = Self::key();
        let entries = self.entries().read();
        if entries.contains_key(&key) {
            return RwLockReadGuard::map(entries, |entries| &entries[&key]);
        }
        drop(entries);
        let mut entries = self.entries().write();
        Self::admit(&mut entries, &key);
        let entries = RwLockWriteGuard::downgrade(entries);
        RwLockReadGuard::map(entries, |entries| &entries[&key])
    }

    pub(crate) fn write(&self) -> MappedRwLockWriteGuard<'_, T> {
        let key = Self::key();
        let mut entries = self.entries().write();
        Self::admit(&mut entries, &key);
        RwLockWriteGuard::map(entries, |entries| entries.entry(key).or_default())
    }
}
