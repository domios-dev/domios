use std::sync::Arc;

use crate::js_runtime::manifest::{EventSourceDecl, NodeSchema};

pub trait EventSourceProvider: Send + Sync {
    fn declarations(&self) -> &[EventSourceDecl];
    fn subscribe(
        &self,
        kind: &str,
        params: &serde_json::Value,
        subscription_id: &str,
        token: &str,
    ) -> Result<(), String>;
    fn unsubscribe(&self, kind: &str, subscription_id: &str) -> Result<(), String>;
}

#[derive(Clone)]
pub struct EventSourceKey {
    plugin_id: String,
    provider: Arc<dyn EventSourceProvider>,
    generation: u64,
}

impl EventSourceKey {
    pub fn provider(&self) -> &dyn EventSourceProvider {
        self.provider.as_ref()
    }

    pub fn provider_handle(&self) -> Arc<dyn EventSourceProvider> {
        self.provider.clone()
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn params_schema(&self, kind: &str) -> Option<NodeSchema> {
        self.provider
            .declarations()
            .iter()
            .find(|decl| decl.kind == kind)
            .and_then(|decl| decl.params.clone())
    }

    pub fn payload_schema(&self, kind: &str) -> Option<NodeSchema> {
        self.provider
            .declarations()
            .iter()
            .find(|decl| decl.kind == kind)
            .and_then(|decl| decl.payload.clone())
    }
}

pub struct EventSourceRegistry {
    entries: parking_lot::RwLock<Vec<Entry>>,
    generation: std::sync::atomic::AtomicU64,
}

impl Default for EventSourceRegistry {
    fn default() -> Self {
        Self {
            entries: parking_lot::RwLock::new(Vec::new()),
            generation: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

struct Entry {
    plugin_id: String,
    provider: Arc<dyn EventSourceProvider>,
    generation: u64,
}

impl EventSourceRegistry {
    pub fn register(
        &self,
        plugin_id: impl Into<String>,
        provider: Arc<dyn EventSourceProvider>,
    ) -> Result<(), String> {
        let plugin_id = plugin_id.into();
        let generation = self
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        let mut entries = self.entries.write();
        if let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry.plugin_id == plugin_id)
        {
            entry.provider = provider;
            entry.generation = generation;
        } else if entries.len() < MAX_EVENT_SOURCE_PROVIDERS {
            entries.push(Entry {
                plugin_id,
                provider,
                generation,
            });
        } else {
            return Err("event source registry is full".into());
        }
        Ok(())
    }

    pub fn unregister(&self, plugin_id: &str) {
        let mut entries = self.entries.write();
        let previous_len = entries.len();
        entries.retain(|entry| entry.plugin_id != plugin_id);
        if previous_len != entries.len() {
            self.generation
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn select_provider(&self, kind: &str) -> Option<EventSourceKey> {
        self.entries
            .read()
            .iter()
            .find(|entry| {
                entry
                    .provider
                    .declarations()
                    .iter()
                    .any(|declaration| declaration.kind == kind)
            })
            .map(|entry| EventSourceKey {
                plugin_id: entry.plugin_id.clone(),
                provider: entry.provider.clone(),
                generation: entry.generation,
            })
    }
}

const MAX_EVENT_SOURCE_PROVIDERS: usize = 256;

#[cfg(test)]
mod tests {
    use super::*;

    struct Provider(Vec<EventSourceDecl>);

    impl EventSourceProvider for Provider {
        fn declarations(&self) -> &[EventSourceDecl] {
            &self.0
        }

        fn subscribe(
            &self,
            _: &str,
            _: &serde_json::Value,
            _: &str,
            _: &str,
        ) -> Result<(), String> {
            Ok(())
        }

        fn unsubscribe(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn source(kind: &str) -> Arc<dyn EventSourceProvider> {
        Arc::new(Provider(vec![EventSourceDecl {
            kind: kind.into(),
            title: kind.into(),
            params: None,
            payload: None,
        }]))
    }

    #[test]
    fn event_source_selects_first_registration_by_declared_kind() {
        let registry = EventSourceRegistry::default();
        registry
            .register("first-plugin", source("shared.message"))
            .unwrap();
        registry
            .register("second-plugin", source("shared.message"))
            .unwrap();

        let selected = registry.select_provider("shared.message").unwrap();
        assert_eq!(selected.plugin_id(), "first-plugin");
        assert!(registry.select_provider("first-plugin").is_none());
    }
}
