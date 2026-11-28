use crate::multiplexer::types::RuntimePromptSource;
use codeterm_storage::{DeskLeafFields, DeskPrompt, MAX_DESK_LEAF_BYTES, MAX_DESK_PROMPT_BYTES};
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::LazyLock;

static WARNED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

const RUNTIME_CONFIG: &str = "/agent_identity/agent_runtime_config";
const SAVED_LAUNCH: &str = "/agent_identity/restorable_launch";
const SYSTEM_PROMPT: &str = "systemPrompt";
const IDENTITY_INSTRUCTIONS: &str = "identity_instructions";
const PROMPT_SOURCE: &str = "runtime_prompt_source";

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
    pub limit: usize,
}

impl std::fmt::Display for OversizedLayout {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} size {} exceeds {} bytes",
            self.fields, self.bytes, self.limit,
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
            limit: MAX_DESK_LEAF_BYTES,
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

fn take_field(leaf: &mut Value, parent: &str, field: &str) -> Option<Value> {
    leaf.pointer_mut(parent)
        .and_then(Value::as_object_mut)
        .and_then(|object| object.remove(field))
}

fn take_text(leaf: &mut Value, parent: &str, field: &str) -> Option<String> {
    match take_field(leaf, parent, field)? {
        Value::String(text) => Some(text),
        _ => None,
    }
}

pub(super) fn bound_leaf(mut leaf: Value) -> Result<DeskLeafFields, OversizedLayout> {
    use codeterm_types::agent::{restore_role, role_kind::builtins, AgentRole};

    let role = restore_role(leaf["role"].as_str(), leaf["role_profile"].as_str());
    let chat_backend = leaf["chatbackend_plugin_id"].is_string();
    let manager = role
        .as_ref()
        .is_some_and(|role| role.role == AgentRole::Manager);
    let general = manager
        && role.as_ref().and_then(|role| role.role_profile.as_deref())
            == Some(builtins::GENERAL_AGENT);
    if general && !chat_backend {
        // General Agent launch renders the current machine prompt from its role.
        take_field(&mut leaf, RUNTIME_CONFIG, SYSTEM_PROMPT);
        take_field(&mut leaf, SAVED_LAUNCH, IDENTITY_INSTRUCTIONS);
    }
    let mut size = check_size("desk leaf fields", &leaf);
    if size.is_err() && chat_backend {
        let source = take_field(&mut leaf, SAVED_LAUNCH, PROMPT_SOURCE);
        let system_prompt = take_text(&mut leaf, RUNTIME_CONFIG, SYSTEM_PROMPT).filter(|prompt| {
            let fingerprint = source
                .as_ref()
                .and_then(|source| source["regenerable"].as_str());
            !general
                && !fingerprint
                    .is_some_and(|digest| digest == RuntimePromptSource::fingerprint(prompt))
        });
        let identity_instructions = take_text(&mut leaf, SAVED_LAUNCH, IDENTITY_INSTRUCTIONS);
        check_size("desk leaf fields", &leaf)?;
        let prompt = DeskPrompt {
            system_prompt,
            identity_instructions,
        };
        if prompt.byte_len() > MAX_DESK_PROMPT_BYTES {
            return Err(OversizedLayout {
                fields: "desk prompt",
                bytes: prompt.byte_len(),
                limit: MAX_DESK_PROMPT_BYTES,
            });
        }
        return Ok(DeskLeafFields {
            values: leaf,
            prompt,
        });
    }
    if manager && !general {
        if size.is_err() && take_field(&mut leaf, RUNTIME_CONFIG, SYSTEM_PROMPT).is_some() {
            size = check_size("desk leaf fields", &leaf);
        }
        if size.is_err() && take_field(&mut leaf, SAVED_LAUNCH, IDENTITY_INSTRUCTIONS).is_some() {
            size = check_size("desk leaf fields", &leaf);
        }
    }
    size.map(|()| DeskLeafFields::new(leaf))
}

pub(super) fn restore_prompt(leaf: &mut Value, prompt: DeskPrompt) {
    if let Some(system_prompt) = prompt.system_prompt {
        leaf["agent_identity"]["restorable_launch"][PROMPT_SOURCE] = serde_json::json!(
            RuntimePromptSource::Preserved(RuntimePromptSource::fingerprint(&system_prompt))
        );
        leaf["agent_identity"]["agent_runtime_config"][SYSTEM_PROMPT] =
            Value::String(system_prompt);
    }
    if let Some(identity) = prompt.identity_instructions {
        leaf["agent_identity"]["restorable_launch"][IDENTITY_INSTRUCTIONS] =
            Value::String(identity);
    }
}

#[cfg(test)]
mod tests;
