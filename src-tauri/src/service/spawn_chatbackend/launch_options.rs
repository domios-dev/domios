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

#[cfg(test)]
mod tests;
