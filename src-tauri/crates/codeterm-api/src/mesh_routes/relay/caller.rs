use codeterm_service_core::service::tab_credentials::TabCaller;
use serde_json::Value;

pub(super) fn bind(
    caller: &TabCaller,
    method: &str,
    path: &str,
    body: &mut Value,
) -> Result<(), String> {
    if !method.eq_ignore_ascii_case("POST") { return Ok(()); }
    let path = path.split('?').next().unwrap_or(path);
    let spawn = path == "/api/agents/spawn";
    let send = path.starts_with("/api/tabs/") && path.ends_with("/send");
    if !spawn && !send { return Ok(()); }
    let body = body.as_object_mut().ok_or("Relay actor body must be an object")?;
    let fields: &[&str] = if spawn { &["spawned_by_tab_id", "creator_tab_id"] } else { &["source_tab"] };
    for field in fields {
        if let Some(claim) = body.get(*field).filter(|value| !value.is_null()) {
            caller.validate_claim(Some(claim.as_str().ok_or("Invalid relay actor claim")?))?;
        }
    }
    let provenance: &[&str] = if spawn { &["origin_peer"] } else { &["source_peer", "system_context"] };
    if provenance.iter().any(|field| body.get(*field).is_some_and(|value| !value.is_null())) {
        return Err("Tab credentials cannot supply relay peer or system provenance".into());
    }
    for field in fields { body.insert((*field).into(), caller.tab_id.clone().into()); }
    Ok(())
}

#[cfg(test)]
mod tests;
