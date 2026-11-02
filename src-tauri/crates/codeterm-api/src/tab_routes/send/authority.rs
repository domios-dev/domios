use super::*;

pub(super) fn bind_source(
    state: &AppState,
    headers: &HeaderMap,
    body: &mut SendTextBody,
    service_push: bool,
) -> Result<(), SendRejection> {
    let remote_agent = if !service_push {
        crate::auth::remote_agent(state, headers).map_err(|(status, message)| SendRejection {
            status,
            body: serde_json::json!({"ok":false,"error":{"code":"forbidden","message":message}}),
        })?
    } else {
        None
    };
    if let Some(agent) = remote_agent {
        if body.system_context.is_some()
            || body
                .source_tab
                .as_ref()
                .is_some_and(|tab| tab != &agent.tab_id)
        {
            return Err(SendRejection::from((
                StatusCode::FORBIDDEN,
                "Remote agents cannot claim another actor or system provenance",
            )));
        }
        body.source_tab = Some(agent.tab_id);
        body.source_peer = Some(
            state
                .mesh
                .lock()
                .paired_peers
                .get(&agent.instance_id)
                .map(|peer| peer.instance_name.clone())
                .unwrap_or(agent.instance_id),
        );
    } else if !service_push
        && crate::auth::caller_peer_id(state, headers).is_none()
        && crate::auth::authenticated_origin(state, headers)
            .is_ok_and(|origin| origin != crate::auth::AuthenticatedOrigin::Mesh)
    {
        body.source_tab = crate::auth::caller_claim(state, headers, body.source_tab.as_deref())
            .map_err(|(status, message)| SendRejection {
            status,
            body: serde_json::json!({"ok":false,"error":{"code":"forbidden","message":message}}),
        })?;
        if crate::auth::tab_caller(state, headers)
            .map_err(|_| SendRejection::from((StatusCode::FORBIDDEN, "Invalid caller credential")))?
            .is_some()
            && (body.source_peer.is_some() || body.system_context.is_some())
        {
            return Err(SendRejection::from((
                StatusCode::FORBIDDEN,
                "Tab credentials cannot supply peer or system provenance",
            )));
        }
    } else if service_push
        && (body.source_tab.is_some()
            || body.source_peer.is_some()
            || body.system_context.is_some())
    {
        return Err(SendRejection::from((
            StatusCode::FORBIDDEN,
            "Service push cannot claim a tab sender",
        )));
    }
    Ok(())
}
