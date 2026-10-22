use codeterm_storage::MAX_DESK_LEAF_BYTES;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

static WARNED: LazyLock<Mutex<HashMap<String, u64>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

const DERIVED_PROMPT: &str = "/agent_identity/agent_runtime_config";
const SAVED_LAUNCH: &str = "/agent_identity/restorable_launch";

fn encoded_len(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

pub(super) fn retain_tabs(snapshot: &super::SessionSnapshot) {
    fn collect<'a>(node: &'a super::SplitNodeSnapshot, ids: &mut HashSet<&'a str>) {
        match node {
            super::SplitNodeSnapshot::Leaf { tab_id, .. } => {
                ids.insert(tab_id);
            }
            super::SplitNodeSnapshot::Split { children, .. } => {
                for child in children {
                    collect(child, ids);
                }
            }
        }
    }
    let mut ids = HashSet::new();
    for workspace in &snapshot.workspaces {
        for tab in &workspace.tabs {
            collect(&tab.split_tree, &mut ids);
        }
    }
    WARNED.lock().retain(|id, _| ids.contains(id.as_str()));
}

fn claim_warning(states: &mut HashMap<String, u64>, tab: &str, fingerprint: Option<u64>) -> bool {
    match fingerprint {
        Some(fingerprint) => {
            if states.get(tab) == Some(&fingerprint) {
                return false;
            }
            states.insert(tab.into(), fingerprint);
            true
        }
        None => {
            states.remove(tab);
            false
        }
    }
}

fn remove_field(leaf: &mut Value, parent: &str, field: &str) -> Option<usize> {
    let object = leaf.pointer_mut(parent)?.as_object_mut()?;
    let removed = object.remove(field)?;
    let bytes = match &removed {
        Value::String(value) => {
            2 + value
                .bytes()
                .map(|byte| match byte {
                    b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 8 | 12 => 2,
                    0..=31 => 6,
                    _ => 1,
                })
                .sum::<usize>()
        }
        _ => encoded_len(&removed),
    };
    Some(bytes + field.len() + 3 + usize::from(!object.is_empty()))
}

// The composed runtime prompt is rebuilt from role and saved identity on restore,
// so dropping it keeps one oversized tab from refusing the whole desk commit.
pub(super) fn bound_leaf(tab_id: &str, mut leaf: Value) -> Value {
    let encoded = serde_json::to_vec(&leaf).unwrap_or_default();
    let mut remaining = encoded.len();
    if remaining <= MAX_DESK_LEAF_BYTES {
        claim_warning(&mut WARNED.lock(), tab_id, None);
        return leaf;
    }
    let mut fingerprint = DefaultHasher::new();
    encoded.hash(&mut fingerprint);
    let warn = claim_warning(&mut WARNED.lock(), tab_id, Some(fingerprint.finish()));
    let prompt_bytes = remove_field(&mut leaf, DERIVED_PROMPT, "systemPrompt");
    remaining -= prompt_bytes.unwrap_or_default();
    let dropped = prompt_bytes.is_some();
    // Only a Worker's saved launch identity carries a persona; others are rebuilt from role.
    let identity_bytes = if remaining > MAX_DESK_LEAF_BYTES
        && leaf.get("role").and_then(Value::as_str) != Some("Worker")
    {
        remove_field(&mut leaf, SAVED_LAUNCH, "identity_instructions")
    } else {
        None
    };
    remaining -= identity_bytes.unwrap_or_default();
    let identity_dropped = identity_bytes.is_some();
    if warn {
        log::warn!(
            "[desk] tab={tab_id} leaf exceeds {MAX_DESK_LEAF_BYTES} bytes; dropped prompt={dropped} identity={identity_dropped} remaining={remaining}"
        );
    }
    leaf
}

#[cfg(test)]
mod tests;
