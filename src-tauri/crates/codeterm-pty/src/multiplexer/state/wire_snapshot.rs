use std::collections::{BTreeMap, HashMap};

use serde::{ser::SerializeMap, Serialize, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{MultiplexerSnapshot, Tab};

#[derive(Serialize)]
struct WireTab<'a> {
    #[serde(flatten)]
    tab: &'a Tab,
    #[serde(skip_serializing_if = "Option::is_none")]
    exited_chat_recipient: Option<&'a crate::multiplexer::types::ExitedChatBinding>,
}

struct WireTabs<'a>(&'a [(&'a str, Tab)]);

impl Serialize for WireTabs<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (id, wire) in self.0 {
            map.serialize_entry(
                id,
                &WireTab {
                    tab: wire,
                    exited_chat_recipient: wire.exited_chat_recipient(),
                },
            )?;
        }
        map.end()
    }
}

/// Key order keeps equal snapshots byte-identical; a fresh map hashes in a new order.
struct SortedMap<'a, V>(&'a HashMap<String, V>);

impl<V: Serialize> Serialize for SortedMap<'_, V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut entries: Vec<_> = self.0.iter().collect();
        entries.sort_unstable_by_key(|(key, _)| *key);
        serializer.collect_map(entries)
    }
}

/// Static per-provider/workspace runtime controls and icons hoisted once per
/// snapshot so tabs carry compact content-addressed references instead of
/// repeating multi-kilobyte arrays and SVGs on every tab.
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeAssets {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    icons: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    authentication: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    commands: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    modes: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    config_options: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    service_tier_availability: BTreeMap<String, Value>,
}

impl RuntimeAssets {
    fn is_empty(&self) -> bool {
        self.icons.is_empty()
            && self.authentication.is_empty()
            && self.commands.is_empty()
            && self.modes.is_empty()
            && self.config_options.is_empty()
            && self.service_tier_availability.is_empty()
    }

    fn compact_tab(&mut self, wire: &mut Tab) {
        if let Some(state) = wire
            .agent_runtime_state
            .as_mut()
            .and_then(Value::as_object_mut)
        {
            if state
                .get("iconSvg")
                .and_then(Value::as_str)
                .is_some_and(|icon| !icon.is_empty())
            {
                if let Some(icon) = state.remove("iconSvg") {
                    let key = intern(&mut self.icons, icon);
                    state.insert("iconRef".into(), Value::String(key));
                }
            }
            if state
                .get("authentication")
                .and_then(Value::as_object)
                .is_some_and(|auth| !auth.is_empty())
            {
                if let Some(auth) = state.remove("authentication") {
                    let key = intern(&mut self.authentication, auth);
                    state.insert("authenticationRef".into(), Value::String(key));
                }
            }
            if let Some(controls) = state.get_mut("controls").and_then(Value::as_object_mut) {
                if controls
                    .get("commands")
                    .and_then(Value::as_array)
                    .is_some_and(|commands| !commands.is_empty())
                {
                    if let Some(commands) = controls.remove("commands") {
                        let key = intern(&mut self.commands, commands);
                        controls.insert("commandsRef".into(), Value::String(key));
                    }
                }
                if controls
                    .get("modes")
                    .and_then(Value::as_array)
                    .is_some_and(|modes| !modes.is_empty())
                {
                    if let Some(modes) = controls.remove("modes") {
                        let key = intern(&mut self.modes, modes);
                        controls.insert("modesRef".into(), Value::String(key));
                    }
                }
                if controls
                    .get("serviceTierAvailability")
                    .and_then(Value::as_object)
                    .is_some_and(|tiers| !tiers.is_empty())
                {
                    if let Some(tiers) = controls.remove("serviceTierAvailability") {
                        let key = intern(&mut self.service_tier_availability, tiers);
                        controls.insert("serviceTierAvailabilityRef".into(), Value::String(key));
                    }
                }
                if controls
                    .get("configOptions")
                    .and_then(Value::as_array)
                    .is_some_and(|options| {
                        !options.is_empty()
                            && options.iter().all(|option| {
                                option
                                    .as_object()
                                    .and_then(|obj| obj.get("currentValue"))
                                    .is_some_and(Value::is_string)
                            })
                    })
                {
                    if let Some(Value::Array(options)) = controls.remove("configOptions") {
                        let mut template = Vec::with_capacity(options.len());
                        let mut values = Vec::with_capacity(options.len());
                        for mut option in options {
                            if let Some(obj) = option.as_object_mut() {
                                if let Some(current) = obj.remove("currentValue") {
                                    values.push(current);
                                }
                            }
                            template.push(option);
                        }
                        let key = intern(&mut self.config_options, Value::Array(template));
                        controls.insert("configOptionsRef".into(), Value::String(key));
                        controls.insert("configOptionValues".into(), Value::Array(values));
                    }
                }
            }
        }
        if let Some(config) = wire
            .agent_runtime_config
            .as_mut()
            .and_then(Value::as_object_mut)
        {
            if config
                .get("serviceTierAvailability")
                .and_then(Value::as_object)
                .is_some_and(|tiers| !tiers.is_empty())
            {
                if let Some(tiers) = config.remove("serviceTierAvailability") {
                    let key = intern(&mut self.service_tier_availability, tiers);
                    config.insert("serviceTierAvailabilityRef".into(), Value::String(key));
                }
            }
        }
    }
}

fn content_hash(value: &Value) -> String {
    let mut hasher = Sha256::new();
    let _ = serde_json::to_writer(&mut hasher, value);
    let digest = hasher.finalize();
    format!(
        "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7]
    )
}

fn intern(bucket: &mut BTreeMap<String, Value>, value: Value) -> String {
    let base = content_hash(&value);
    let mut candidate = base.clone();
    let mut suffix = 1u32;
    loop {
        match bucket.get(&candidate) {
            Some(existing) if existing == &value => return candidate,
            Some(_) => {
                candidate = format!("{base}-{suffix}");
                suffix += 1;
            }
            None => {
                bucket.insert(candidate.clone(), value);
                return candidate;
            }
        }
    }
}

fn prepare_wire_tab(tab: &Tab, assets: &mut RuntimeAssets) -> Tab {
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
        .and_then(Value::as_object_mut)
    {
        config.remove("systemPrompt");
    }
    let overflow = wire.prior_session_binding_nonces.len().saturating_sub(2);
    wire.prior_session_binding_nonces.drain(..overflow);
    assets.compact_tab(&mut wire);
    wire
}

impl Serialize for MultiplexerSnapshot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut assets = RuntimeAssets::default();
        let mut entries: Vec<_> = self.tabs.iter().collect();
        entries.sort_unstable_by_key(|(id, _)| *id);
        let wire_tabs: Vec<(&str, Tab)> = entries
            .into_iter()
            .map(|(id, tab)| (id.as_str(), prepare_wire_tab(tab, &mut assets)))
            .collect();

        let field_count = 5
            + usize::from(!self.teams.is_empty())
            + usize::from(!self.recently_closed.is_empty())
            + usize::from(!self.closed_structures.is_empty())
            + usize::from(!self.tab_statuses.is_empty())
            + usize::from(!assets.is_empty());

        let mut map = serializer.serialize_map(Some(field_count))?;
        map.serialize_entry("generation", &self.generation)?;
        map.serialize_entry("workspaces", &self.workspaces)?;
        map.serialize_entry("tabs", &WireTabs(&wire_tabs))?;
        if !self.teams.is_empty() {
            map.serialize_entry("teams", &self.teams)?;
        }
        map.serialize_entry("active_workspace_id", &self.active_workspace_id)?;
        map.serialize_entry("focused_tab_id", &self.focused_tab_id)?;
        if !self.recently_closed.is_empty() {
            map.serialize_entry("recently_closed", &self.recently_closed)?;
        }
        if !self.closed_structures.is_empty() {
            map.serialize_entry("closed_structures", &self.closed_structures)?;
        }
        if !self.tab_statuses.is_empty() {
            map.serialize_entry("tab_statuses", &SortedMap(&self.tab_statuses))?;
        }
        if !assets.is_empty() {
            map.serialize_entry("runtime_assets", &assets)?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests;
