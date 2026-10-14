use crate::service::{permission_policy, ServiceContext, SpawnAgentParams};

fn permission_request(requested: Option<bool>, authorized: bool) -> bool {
    requested.unwrap_or(authorized) && authorized
}

pub(super) fn runtime_launch_mode(full_access: bool) -> &'static str {
    if full_access {
        "full-access"
    } else {
        "auto-accept-edits"
    }
}

pub(super) fn authorize(
    svc: &ServiceContext,
    provider: &str,
    params: Option<&SpawnAgentParams>,
) -> Result<SpawnAgentParams, String> {
    let mut params = params.cloned().unwrap_or_default();
    let carried = params
        .permission_bypass_authorization
        .take()
        .filter(|authorization| authorization.authorizes(provider));
    let authorization = if params.skip_permissions == Some(false) || carried.is_some() {
        carried
    } else {
        permission_policy::configured_permission_bypass_for(
            &svc.app_handle,
            &svc.agent_registry,
            provider,
        )?
    };
    params.skip_permissions = Some(permission_request(
        params.skip_permissions,
        authorization.is_some(),
    ));
    params.permission_bypass_authorization = authorization;
    Ok(params)
}

pub(super) fn apply_prepared(
    mut config: serde_json::Value,
    provider: &str,
    nonce: Option<&str>,
    options: Option<&crate::multiplexer::types::RestorableRuntimeLaunchOptions>,
) -> Result<serde_json::Value, String> {
    let Some(options) = options else {
        return Ok(config);
    };
    if options.provider_id != provider || nonce != Some(options.binding_nonce.as_str()) {
        return Err("Runtime launch options binding changed".into());
    }
    if config.is_null() {
        config = serde_json::json!({});
    }
    let object = config
        .as_object_mut()
        .ok_or("Runtime spawn config must be an object")?;
    object.insert("mode".into(), serde_json::json!(options.mode));
    object.insert(
        "additionalDirectories".into(),
        serde_json::json!(options.additional_directories),
    );
    if let Some(access) = options.network_access {
        object.insert("networkAccess".into(), serde_json::json!(access));
    } else {
        object.remove("networkAccess");
    }
    Ok(config)
}

#[cfg(test)]
#[path = "launch_options/tests.rs"]
mod tests;
