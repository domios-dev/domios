use std::collections::HashMap;

use serde::{ser::SerializeMap, Serialize, Serializer};

use super::Tab;
use crate::multiplexer::types::ClosedTab;

pub(super) fn tabs<S: Serializer>(
    tabs: &HashMap<String, Tab>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut entries: Vec<_> = tabs.iter().collect();
    entries.sort_unstable_by_key(|(id, _)| *id);
    let mut map = serializer.serialize_map(Some(entries.len()))?;
    for (id, tab) in entries {
        let mut wire = tab.clone();
        if let Some(mode) = &tab.restorable_launch.execution_mode {
            let config = wire
                .agent_runtime_config
                .get_or_insert_with(|| serde_json::json!({}));
            config["executionMode"] = serde_json::json!(mode);
        }
        if let Some(config) = wire
            .agent_runtime_config
            .as_mut()
            .and_then(serde_json::Value::as_object_mut)
        {
            config.remove("systemPrompt");
        }
        let overflow = wire.prior_session_binding_nonces.len().saturating_sub(2);
        wire.prior_session_binding_nonces.drain(..overflow);
        map.serialize_entry(id, &wire)?;
    }
    map.end()
}

/// Key order keeps equal snapshots byte-identical; a fresh map hashes in a new order.
pub(super) fn sorted<S: Serializer, V: Serialize>(
    values: &HashMap<String, V>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut entries: Vec<_> = values.iter().collect();
    entries.sort_unstable_by_key(|(key, _)| *key);
    serializer.collect_map(entries)
}

pub(super) fn closed_tab<S: Serializer>(tab: &ClosedTab, serializer: S) -> Result<S::Ok, S::Error> {
    let mut wire = serde_json::to_value(tab).map_err(serde::ser::Error::custom)?;
    if let Some(fields) = wire.as_object_mut() {
        fields.remove("restorable_launch");
    }
    wire.serialize(serializer)
}

#[cfg(test)]
mod tests;
