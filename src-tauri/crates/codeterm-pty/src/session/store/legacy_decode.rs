use super::{PersistedAgentSessionIdentity, SessionSnapshot};
use codeterm_storage::{DeskAgentSessionIdentity, DeskLayoutNode, DeskLeafFields};
use std::collections::HashMap;

pub struct DecodedSnapshot {
    pub snapshot: SessionSnapshot,
    pub layouts: Vec<Vec<DeskLayoutNode>>,
}

pub fn decode_snapshot(json: &str) -> Result<DecodedSnapshot, String> {
    let mut value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| error.to_string())?;
    codeterm_types::startup_records::normalize_pane_id_fields(&mut value)
        .map_err(|error| error.to_string())?;
    let snapshot = serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
    let layouts = decode_layouts(&value)?;
    Ok(DecodedSnapshot { snapshot, layouts })
}

fn decode_layouts(value: &serde_json::Value) -> Result<Vec<Vec<DeskLayoutNode>>, String> {
    value
        .get("workspaces")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "legacy snapshot has no workspace array".to_string())?
        .iter()
        .map(|workspace| {
            workspace
                .get("tabs")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| "legacy workspace has no tab array".to_string())?
                .iter()
                .map(|tab| {
                    let tree = tab
                        .get("split_tree")
                        .ok_or_else(|| "legacy tab has no split tree".to_string())?;
                    decode_layout(tree)
                })
                .collect()
        })
        .collect()
}

fn decode_layout(value: &serde_json::Value) -> Result<DeskLayoutNode, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "legacy layout node is not an object".to_string())?;
    match object.get("type").and_then(serde_json::Value::as_str) {
        Some("Leaf") => {
            let tab_id = object
                .get("tab_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "legacy layout leaf has no tab id".to_string())?
                .to_owned();
            let cwd = match object.get("cwd") {
                Some(serde_json::Value::Null) | None => None,
                Some(serde_json::Value::String(cwd)) => Some(cwd.clone()),
                _ => return Err("legacy layout cwd is not a string or null".to_string()),
            };
            let mut fields = object.clone();
            fields.remove("type");
            fields.remove("tab_id");
            Ok(DeskLayoutNode::Leaf {
                tab_id,
                cwd,
                fields: DeskLeafFields {
                    values: serde_json::Value::Object(fields),
                },
            })
        }
        Some("Split") => {
            let orientation = object
                .get("orientation")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "legacy split has no orientation".to_string())?
                .to_owned();
            let ratio = object
                .get("ratio")
                .and_then(serde_json::Value::as_f64)
                .ok_or_else(|| "legacy split has no numeric ratio".to_string())?;
            let children = object
                .get("children")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| "legacy split has no children array".to_string())?
                .iter()
                .map(decode_layout)
                .collect::<Result<Vec<_>, _>>()?;
            let mut fields = object.clone();
            fields.remove("type");
            fields.remove("orientation");
            fields.remove("ratio");
            fields.remove("children");
            Ok(DeskLayoutNode::Split {
                orientation,
                ratio,
                children,
                fields,
            })
        }
        _ => Err("legacy layout node has an unknown type".to_string()),
    }
}

pub fn decode_identity_catalog(
    json: Option<&str>,
) -> Result<Vec<DeskAgentSessionIdentity>, String> {
    let Some(json) = json else {
        return Ok(Vec::new());
    };
    let catalog: IdentityCatalog = serde_json::from_str(json).map_err(|error| error.to_string())?;
    catalog
        .sessions
        .into_iter()
        .map(|(key, identity)| {
            let (provider, session_id) = decode_identity_key(&key)?;
            Ok(DeskAgentSessionIdentity {
                provider,
                session_id,
                record_provider: identity.provider,
                role: identity.role,
                role_profile: identity.role_profile,
                parent_tab_id: identity.parent_tab_id,
                can_spawn: identity.can_spawn,
            })
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct IdentityCatalog {
    #[serde(default)]
    sessions: HashMap<String, PersistedAgentSessionIdentity>,
}

fn decode_identity_key(key: &str) -> Result<(String, String), String> {
    let (length, payload) = key
        .split_once(':')
        .ok_or_else(|| "agent identity key has no provider length".to_string())?;
    let length: usize = length
        .parse()
        .map_err(|_| "agent identity key has invalid provider length".to_string())?;
    let provider = payload
        .get(..length)
        .ok_or_else(|| "agent identity key provider is truncated".to_string())?;
    let session_id = payload
        .get(length..)
        .ok_or_else(|| "agent identity key session is invalid".to_string())?;
    if provider.is_empty() || session_id.is_empty() {
        return Err("agent identity key has an empty component".into());
    }
    Ok((provider.to_owned(), session_id.to_owned()))
}
