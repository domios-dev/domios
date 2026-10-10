use crate::js_runtime::manifest::{EventSourceDecl, GraphNodesCap};
use parking_lot::RwLock;

const MAX_GRAPH_NODE_PLUGINS: usize = 256;

#[derive(Default)]
pub struct GraphNodeRegistry {
    entries: RwLock<Vec<(String, Vec<EventSourceDecl>)>>,
}

impl GraphNodeRegistry {
    pub fn register(&self, id: impl Into<String>, nodes: &GraphNodesCap) -> Result<(), String> {
        let id = id.into();
        let mut entries = self.entries.write();
        let existing = entries.iter().position(|(registered, _)| registered == &id);
        if existing.is_none() && entries.len() >= MAX_GRAPH_NODE_PLUGINS {
            return Err("graph node capability registry is full".into());
        }
        let value = (id, nodes.event_sources.clone());
        if let Some(index) = existing {
            entries[index] = value;
        } else {
            entries.push(value);
        }
        Ok(())
    }

    pub fn unregister(&self, id: &str) {
        self.entries
            .write()
            .retain(|(registered, _)| registered != id);
    }

    pub fn event_sources(&self) -> Vec<EventSourceDecl> {
        self.entries
            .read()
            .iter()
            .flat_map(|(_, sources)| sources.iter().cloned())
            .collect()
    }
}
