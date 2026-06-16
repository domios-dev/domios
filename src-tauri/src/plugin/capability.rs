//! Generic runtime registry mapping a capability id to its implementation.
//!
//! Populated by the [`PluginManager`](super::PluginManager) as plugins activate
//! and deactivate. Core code always resolves a capability through a registry —
//! never a hardcoded id list or display-string match. This is the seam that lets
//! a plugin contribute an agent provider, a secret backend, a transcriber, or a
//! future capability without core knowing the plugin's identity.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

/// A thread-safe `id -> Arc<dyn Capability>` map.
///
/// `T` is a capability trait object (e.g. `dyn Transcriber`). Cloning an entry is
/// cheap (`Arc`); the lock is held only for the map operation, never across the
/// returned value's use.
pub struct CapabilityRegistry<T: ?Sized> {
    entries: RwLock<HashMap<String, Arc<T>>>,
}

impl<T: ?Sized> CapabilityRegistry<T> {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Insert or replace the implementation for `id`.
    pub fn register(&self, id: impl Into<String>, imp: Arc<T>) {
        self.entries.write().insert(id.into(), imp);
    }

    /// Remove the implementation for `id`, if present.
    pub fn unregister(&self, id: &str) {
        self.entries.write().remove(id);
    }

    /// Resolve the implementation for `id`.
    pub fn get(&self, id: &str) -> Option<Arc<T>> {
        self.entries.read().get(id).cloned()
    }

    /// All registered implementations (order unspecified).
    pub fn active(&self) -> Vec<Arc<T>> {
        self.entries.read().values().cloned().collect()
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
mod tests {
    use super::*;

    trait Greeter: Send + Sync {
        fn hi(&self) -> String;
    }
    struct A;
    impl Greeter for A {
        fn hi(&self) -> String {
            "a".into()
        }
    }
    struct B;
    impl Greeter for B {
        fn hi(&self) -> String {
            "b".into()
        }
    }

    #[test]
    fn register_get_unregister() {
        let r: CapabilityRegistry<dyn Greeter> = CapabilityRegistry::new();
        assert!(r.get("a").is_none());
        r.register("a", Arc::new(A));
        assert_eq!(r.get("a").unwrap().hi(), "a");
        assert!(r.contains("a"));
        assert_eq!(r.active().len(), 1);
        assert_eq!(r.ids(), vec!["a".to_string()]);
        r.unregister("a");
        assert!(r.get("a").is_none());
        assert!(!r.contains("a"));
    }

    #[test]
    fn register_replaces_same_id() {
        let r: CapabilityRegistry<dyn Greeter> = CapabilityRegistry::new();
        r.register("g", Arc::new(A));
        r.register("g", Arc::new(B));
        assert_eq!(r.get("g").unwrap().hi(), "b");
        assert_eq!(r.active().len(), 1);
    }
}
