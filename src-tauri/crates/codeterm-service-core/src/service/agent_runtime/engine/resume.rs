use super::*;

fn unused(config: &Value, thread: &str) -> bool {
    config["unusedThreadId"].as_str() == Some(thread)
}

pub(super) fn mark_unused(
    svc: &ServiceContext,
    session: &Session,
    thread: &str,
) -> Result<(), RuntimeFailure> {
    validate_binding(svc, session)?;
    let context = session.context.lock().clone();
    let mut mux = svc.mux.write();
    let tab = mux
        .get_tab_mut(&context.tab_id)
        .ok_or(RuntimeFailure::Superseded)?;
    if tab.agent_session_id.as_deref() != Some(thread)
        || tab.session_binding_nonce.as_deref() != Some(&context.binding_nonce)
    {
        return Err(RuntimeFailure::Superseded);
    }
    let config = tab
        .agent_runtime_config
        .get_or_insert_with(|| serde_json::json!({}));
    config["unusedThreadId"] = Value::String(thread.into());
    Ok(())
}

pub(super) async fn before_turn(
    svc: &ServiceContext,
    session: &Session,
) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    let changed = {
        let mut mux = svc.mux.write();
        let tab = mux
            .get_tab_mut(&context.tab_id)
            .ok_or(RuntimeFailure::Superseded)?;
        if tab.session_binding_nonce.as_deref() != Some(&context.binding_nonce)
            || tab.agent_session_id != context.thread_id
        {
            return Err(RuntimeFailure::Superseded);
        }
        if let Some(config) = tab
            .agent_runtime_config
            .as_mut()
            .and_then(Value::as_object_mut)
        {
            if let Some(thread) = config.remove("unusedThreadId") {
                config.insert("usedThreadId".into(), thread);
            }
            config.contains_key("usedThreadId")
        } else {
            false
        }
    };
    if changed {
        let owner = svc.clone();
        tokio::task::spawn_blocking(move || {
            let outcome =
                crate::service::session_persist::write_session_snapshot(&owner, &owner.mux)?;
            if !matches!(outcome, crate::session::store::SaveOutcome::Saved) {
                return Err("Conversation usage persistence refused".to_string());
            }
            Ok(())
        })
        .await
        .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))?
        .map_err(RuntimeFailure::HookFailed)?;
        let mut mux = svc.mux.write();
        if let Some(tab) = mux.get_tab_mut(&context.tab_id) {
            if tab.session_binding_nonce.as_deref() == Some(&context.binding_nonce) {
                if let Some(config) = tab
                    .agent_runtime_config
                    .as_mut()
                    .and_then(Value::as_object_mut)
                {
                    config.remove("usedThreadId");
                }
            }
        }
    }
    Ok(())
}

pub(super) async fn load(
    svc: &ServiceContext,
    provider: &str,
    session: &Arc<Session>,
    process: &Arc<Process>,
    operation: RuntimeOperation,
) -> Result<RuntimeResult, RuntimeFailure> {
    let result = run(svc, provider, session, process, operation.clone()).await;
    let RuntimeOperation::ResumeThread {
        thread_id,
        instructions,
        settings,
    } = operation
    else {
        return result;
    };
    if !matches!(result, Err(RuntimeFailure::ConversationMissing)) {
        return result;
    }
    validate_binding(svc, session)?;
    let context = session.context.lock().clone();
    {
        let mut mux = svc.mux.write();
        let tab = mux
            .get_tab_mut(&context.tab_id)
            .ok_or(RuntimeFailure::Superseded)?;
        if tab.session_binding_nonce.as_deref() != Some(&context.binding_nonce)
            || tab.agent_session_id.as_deref() != Some(&thread_id)
        {
            return Err(RuntimeFailure::Superseded);
        }
        if !tab
            .agent_runtime_config
            .as_ref()
            .is_some_and(|config| unused(config, &thread_id))
        {
            return result;
        }
        tab.agent_session_id = None;
        tab.agent_session_name = None;
        tab.restored_from_snapshot = false;
        if let crate::multiplexer::types::TabBacking::ChatBackend { session_id, .. } =
            &mut tab.backing
        {
            session_id.clear();
        }
        session.context.lock().thread_id = None;
        session.state.lock().thread_id = None;
    }
    run(
        svc,
        provider,
        session,
        process,
        RuntimeOperation::OpenThread {
            instructions,
            settings,
        },
    )
    .await
}

#[cfg(test)]
mod tests;
