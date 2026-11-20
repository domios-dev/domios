use codeterm_storage::MAX_DESK_LEAF_BYTES;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::LazyLock;

static WARNED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

const RUNTIME_CONFIG: &str = "/agent_identity/agent_runtime_config";
const SAVED_LAUNCH: &str = "/agent_identity/restorable_launch";

pub(super) fn retain_tabs(snapshot: &super::SessionSnapshot) {
    let ids: HashSet<&str> = snapshot
        .workspaces
        .iter()
        .flat_map(|workspace| workspace.tabs.iter().map(|tab| tab.id.as_str()))
        .collect();
    WARNED.lock().retain(|id| ids.contains(id.as_str()));
}

pub(super) fn report_omission(tab_id: &str, error: OversizedLayout) {
    if WARNED.lock().insert(tab_id.into()) {
        log::warn!("[desk] tab={tab_id} omitted from snapshot: {error}");
    }
}

pub(super) fn clear_omission(tab_id: &str) {
    WARNED.lock().remove(tab_id);
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct OversizedLayout {
    pub fields: &'static str,
    pub bytes: usize,
}

impl std::fmt::Display for OversizedLayout {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} size {} exceeds {MAX_DESK_LEAF_BYTES} bytes",
            self.fields, self.bytes
        )
    }
}

pub(super) fn check_size(
    fields: &'static str,
    value: &impl serde::Serialize,
) -> Result<(), OversizedLayout> {
    let mut size = JsonSize(0);
    serde_json::to_writer(&mut size, value).expect("infallible JSON field size counter");
    if size.0 > MAX_DESK_LEAF_BYTES {
        return Err(OversizedLayout {
            fields,
            bytes: size.0,
        });
    }
    Ok(())
}

struct JsonSize(usize);

impl std::io::Write for JsonSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn remove_field(leaf: &mut Value, parent: &str, field: &str) -> bool {
    leaf.pointer_mut(parent)
        .and_then(Value::as_object_mut)
        .and_then(|object| object.remove(field))
        .is_some()
}

pub(super) fn bound_leaf(mut leaf: Value) -> Result<Value, OversizedLayout> {
    use codeterm_types::agent::{restore_role, role_kind::builtins, AgentRole};

    let role = restore_role(leaf["role"].as_str(), leaf["role_profile"].as_str());
    let manager = leaf["chatbackend_plugin_id"].is_null()
        && role
            .as_ref()
            .is_some_and(|role| role.role == AgentRole::Manager);
    let general = manager
        && role.as_ref().and_then(|role| role.role_profile.as_deref())
            == Some(builtins::GENERAL_AGENT);
    if general {
        // General Agent launch renders the current machine prompt from its role.
        remove_field(&mut leaf, RUNTIME_CONFIG, "systemPrompt");
        remove_field(&mut leaf, SAVED_LAUNCH, "identity_instructions");
    }
    let mut size = check_size("desk leaf fields", &leaf);
    if manager && !general {
        if size.is_err() && remove_field(&mut leaf, RUNTIME_CONFIG, "systemPrompt") {
            size = check_size("desk leaf fields", &leaf);
        }
        if size.is_err() && remove_field(&mut leaf, SAVED_LAUNCH, "identity_instructions") {
            size = check_size("desk leaf fields", &leaf);
        }
    }
    size.map(|()| leaf)
}

#[cfg(test)]
mod tests;
