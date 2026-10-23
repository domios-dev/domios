use serde_json::{json, Map, Value};

pub(super) fn equal_except(a: &Value, b: &Value, ignored: &[&str]) -> bool {
    match (a.as_object(), b.as_object()) {
        (Some(a), Some(b)) => {
            a.iter()
                .filter(|(key, _)| !ignored.contains(&key.as_str()))
                .all(|(key, value)| b.get(key) == Some(value))
                && b.keys()
                    .filter(|key| !ignored.contains(&key.as_str()))
                    .all(|key| a.contains_key(key))
        }
        _ => a == b,
    }
}

pub(super) fn event(base: u64, before: &Value, generation: u64, after: &Value) -> Value {
    status_changes(before, after).map_or_else(
        || json!({"generation": generation}),
        |tabs| json!({"generation": generation, "base_generation": base, "status_deltas": tabs}),
    )
}

fn status_changes(before: &Value, after: &Value) -> Option<Map<String, Value>> {
    if !equal_except(before, after, &["generation", "tab_statuses", "tabs"]) {
        return None;
    }
    let old_tabs = before.get("tabs")?.as_object()?;
    let new_tabs = after.get("tabs")?.as_object()?;
    if old_tabs.len() != new_tabs.len() {
        return None;
    }
    let mut changes = Map::new();
    for (id, tab) in new_tabs {
        let old_tab = old_tabs.get(id)?;
        if !equal_except(old_tab, tab, &["agent_state"]) {
            return None;
        }
        let old_status = before
            .get("tab_statuses")
            .and_then(|statuses| statuses.get(id));
        let status = after
            .get("tab_statuses")
            .and_then(|statuses| statuses.get(id));
        if old_status != status || old_tab.get("agent_state") != tab.get("agent_state") {
            changes.insert(
                id.clone(),
                json!({"status": status, "agent_state": tab.get("agent_state")}),
            );
        }
    }
    Some(changes)
}
