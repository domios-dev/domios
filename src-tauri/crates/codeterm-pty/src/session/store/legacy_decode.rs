use super::{PersistedAgentSessionIdentity, SessionSnapshot};
use codeterm_storage::DeskAgentSessionIdentity;
use std::collections::HashMap;

pub(super) fn decode_snapshot(json: &str) -> Result<SessionSnapshot, String> {
    let mut value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| error.to_string())?;
    codeterm_types::startup_records::normalize_pane_id_fields(&mut value)
        .map_err(|error| error.to_string())?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

pub(super) fn decode_identity_catalog(
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
