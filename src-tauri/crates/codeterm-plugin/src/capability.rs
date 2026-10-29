//! Generic runtime registry mapping a capability id to its implementation,
//! populated as plugins activate/deactivate. Core always resolves through a
//! registry, never a hardcoded id list — the seam that lets a plugin contribute
//! a capability without core knowing its identity.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

type RegistrationHook<T> = Arc<dyn Fn(&str, &Arc<T>) + Send + Sync>;

/// A thread-safe capability map; locks are dropped before implementations are called.
pub struct CapabilityRegistry<T: ?Sized> {
    entries: RwLock<HashMap<String, (Arc<T>, String)>>,
    prepare: Option<RegistrationHook<T>>,
}

impl<T: ?Sized> CapabilityRegistry<T> {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            prepare: None,
        }
    }

    pub fn with_registration_hook(prepare: impl Fn(&str, &Arc<T>) + Send + Sync + 'static) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            prepare: Some(Arc::new(prepare)),
        }
    }

    /// Insert or replace the implementation for `id`.
    pub fn register(&self, id: impl Into<String>, imp: Arc<T>) {
        let id = id.into();
        if let Some(prepare) = &self.prepare {
            prepare(&id, &imp);
        }
        self.entries
            .write()
            .insert(id, (imp, uuid::Uuid::new_v4().simple().to_string()));
    }

    /// Remove the implementation for `id`, if present.
    pub fn unregister(&self, id: &str) {
        self.entries.write().remove(id);
    }

    /// Resolve the implementation for `id`.
    pub fn get(&self, id: &str) -> Option<Arc<T>> {
        self.entries.read().get(id).map(|entry| entry.0.clone())
    }

    /// Host snapshots fence replacement even when no document is currently registered.
    pub fn registration_id(&self, id: &str) -> Option<String> {
        self.entries.read().get(id).map(|entry| entry.1.clone())
    }

    /// All registered implementations (order unspecified).
    pub fn active(&self) -> Vec<Arc<T>> {
        self.entries
            .read()
            .values()
            .map(|entry| entry.0.clone())
            .collect()
    }

    /// All registered ids (order unspecified).
    pub fn ids(&self) -> Vec<String> {
        self.entries.read().keys().cloned().collect()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.read().contains_key(id)
    }
}

impl<T: ?Sized> Default for CapabilityRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod registration_tests;
