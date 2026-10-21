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
    entries: RwLock<HashMap<String, Arc<T>>>,
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
        self.entries.write().insert(id, imp);
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

#[cfg(test)]
mod registration_tests {
    use super::*;
    #[test]
    fn capability_registration_prepares_initial_and_reloaded_implementations() {
        let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let seen = events.clone();
        let registry = CapabilityRegistry::<String>::with_registration_hook(move |id, value| {
            seen.lock().push((id.to_owned(), value.as_str().to_owned()))
        });
        registry.register("fixture", Arc::new("first".into()));
        registry.register("fixture", Arc::new("second".into()));
        assert_eq!(
            *events.lock(),
            vec![
                ("fixture".into(), "first".into()),
                ("fixture".into(), "second".into())
            ]
        );
        assert_eq!(registry.get("fixture").unwrap().as_str(), "second");
    }
}
