#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteTabOwner {
    pub peer_id: String,
    pub tab_id: String,
}

pub fn encode_remote_tab_component(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

pub fn parse_remote_tab_owner(ordinary_id: &str) -> Result<RemoteTabOwner, &'static str> {
    let value = ordinary_id
        .strip_prefix("mesh:")
        .ok_or("remote tab identity required")?;
    let (peer_id, tab_id) = value.split_once('/').ok_or("invalid remote tab identity")?;
    let invalid = |value: &str| {
        value.is_empty()
            || value
                .chars()
                .any(|c| c.is_whitespace() || "/?&#".contains(c))
    };
    if invalid(peer_id) || invalid(tab_id) {
        return Err("invalid remote tab identity");
    }
    Ok(RemoteTabOwner {
        peer_id: peer_id.to_string(),
        tab_id: tab_id.to_string(),
    })
}

pub fn validate_remote_stream_owner(
    peer_id: &str,
    ordinary_id: &str,
    path: &str,
) -> Result<RemoteTabOwner, &'static str> {
    if path != "/api/output/stream" && path != "/api/events/stream" {
        return Err("tab-scoped relay requires tab stream");
    }
    let owner = parse_remote_tab_owner(ordinary_id)?;
    if owner.peer_id != peer_id {
        return Err("remote tab owner mismatch");
    }
    Ok(owner)
}

pub fn route_remote_tab_action(
    ordinary_id: &str,
    authenticated_peer: &str,
    peer_connected: bool,
    method: &str,
    suffix: &str,
) -> Result<crate::mesh::types::RelayCommand, &'static str> {
    let owner = parse_remote_tab_owner(ordinary_id)?;
    if owner.peer_id != authenticated_peer {
        return Err("remote tab owner mismatch");
    }
    if !peer_connected {
        return Err("remote tab unavailable");
    }
    let delivery_id = (method.eq_ignore_ascii_case("POST") && suffix == "/send")
        .then(|| uuid::Uuid::new_v4().to_string());
    let tab = encode_remote_tab_component(&owner.tab_id);
    let path = match suffix {
        "/inspect" => format!("/api/tabs/inspect?tab_ids={tab}"),
        "/stream" => "/api/output/stream".to_string(),
        _ => format!("/api/tabs/{tab}{suffix}"),
    };
    Ok(crate::mesh::types::RelayCommand {
        method: method.to_string(),
        path,
        body: serde_json::Value::Null,
        delivery_id,
    })
}

#[cfg(test)]
mod tests;
