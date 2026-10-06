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
pub struct EventSourceKey(Arc<dyn EventSourceProvider>);

impl EventSourceKey {
    pub fn provider(&self) -> &dyn EventSourceProvider {
        self.0.as_ref()
    }

    pub fn params_schema(&self, kind: &str) -> Option<NodeSchema> {
        self.0
            .declarations()
            .iter()
            .find(|decl| decl.kind == kind)
            .and_then(|decl| decl.params.clone())
    }
}

#[derive(Default)]
pub struct EventSourceRegistry {
    entries: parking_lot::RwLock<Vec<Entry>>,
}

struct Entry {
    plugin_id: String,
    provider: Arc<dyn EventSourceProvider>,
}

impl EventSourceRegistry {
    pub fn register(
        &self,
        plugin_id: impl Into<String>,
        provider: Arc<dyn EventSourceProvider>,
    ) -> Result<(), String> {
        let plugin_id = plugin_id.into();
        let mut entries = self.entries.write();
        if let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry.plugin_id == plugin_id)
        {
            entry.provider = provider;
        } else if entries.len() < MAX_EVENT_SOURCE_PROVIDERS {
            entries.push(Entry {
                plugin_id,
                provider,
            });
        } else {
            return Err("event source registry is full".into());
        }
        Ok(())
    }

    pub fn unregister(&self, plugin_id: &str) {
        self.entries
            .write()
            .retain(|entry| entry.plugin_id != plugin_id);
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
            .map(|entry| EventSourceKey(entry.provider.clone()))
    }
}

const MAX_EVENT_SOURCE_PROVIDERS: usize = 256;
