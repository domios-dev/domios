use std::sync::Arc;

use parking_lot::Mutex;

use crate::SecretStore;

type RegisterListener = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Default)]
/// Plugin backends with a shared initialization flight for each registration.
pub struct SecretRegistry {
    stores: codeterm_plugin::capability::CapabilityRegistry<dyn SecretStore>,
    listeners: Mutex<Vec<RegisterListener>>,
}

impl SecretRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_register(&self, listener: RegisterListener) {
        self.listeners.lock().push(listener);
    }

    pub fn register(&self, id: impl Into<String>, store: Arc<dyn SecretStore>) {
        let id = id.into();
        self.stores
            .register(id.clone(), crate::readiness::wrap(store));
        let listeners = self.listeners.lock().clone();
        for listener in listeners {
            listener(&id);
        }
    }

    pub fn unregister(&self, id: &str) {
        self.stores.unregister(id);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn SecretStore>> {
        self.stores.get(id)
    }

    pub fn active(&self) -> Vec<Arc<dyn SecretStore>> {
        self.stores.active()
    }

    pub fn ids(&self) -> Vec<String> {
        self.stores.ids()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.stores.contains(id)
    }
}
