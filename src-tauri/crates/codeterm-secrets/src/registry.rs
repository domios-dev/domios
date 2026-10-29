use std::sync::Arc;

use crate::SecretStore;

#[derive(Default)]
/// Plugin backends with a shared initialization flight for each registration.
pub struct SecretRegistry {
    stores: codeterm_plugin::capability::CapabilityRegistry<dyn SecretStore>,
}

impl SecretRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, id: impl Into<String>, store: Arc<dyn SecretStore>) {
        self.stores.register(id, crate::readiness::wrap(store));
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
