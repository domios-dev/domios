use super::*;

#[cfg(any(test, feature = "test-support"))]
pub(crate) struct ExpectedLaunchCommit {
    pub stage: &'static str,
    pub callback: Mutex<Option<Box<dyn FnOnce(&ServiceContext) + Send>>>,
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn expected_launch_commit(svc: &ServiceContext, stage: &str) {
    if let Some(boundary) = svc.try_ext::<ExpectedLaunchCommit>().filter(|boundary| boundary.stage == stage) {
        let callback = boundary.callback.lock().take();
        if let Some(callback) = callback { callback(svc); }
    }
}

pub(super) async fn ensure(
    svc: &ServiceContext,
    tab: &str,
    fork: Option<(String, String)>,
) -> Result<Arc<Session>, RuntimeFailure> {
    ensure_account(svc, tab, fork, None).await
}

pub(crate) async fn ensure_expected(svc: &ServiceContext, tab: &str, expected_nonce: &str, fork: Option<(&str, &str)>) -> Result<Arc<Session>, RuntimeFailure> {
    ensure_account(svc, tab, fork.map(|(thread, turn)| (thread.into(), turn.into())), Some(expected_nonce.to_owned())).await
}

async fn ensure_account(svc: &ServiceContext, tab: &str, fork: Option<(String, String)>, expected_nonce: Option<String>) -> Result<Arc<Session>, RuntimeFailure> {
    check_expected_nonce(svc, tab, expected_nonce.as_deref())?;
    let authority = if expected_nonce.is_some() { None } else { crate::service::messenger::switch_handoff::launch_authority(svc, tab) };
    crate::service::messenger::switch_handoff::scope_launch(authority.clone(), async {
    let expected = match &authority {
        Some(authority) => { authority.validate(svc).map_err(|_| RuntimeFailure::Superseded)?; authority.snapshot() },
        None => svc.mux.read().get_tab(tab).filter(|current| expected_nonce.as_deref().is_none_or(|nonce| current.session_binding_nonce.as_deref() == Some(nonce))).cloned().ok_or(RuntimeFailure::Superseded)?,
    };
    if expected.frozen {
        return Err(RuntimeFailure::Refused(
            "Tab is sleeping; open or address it to wake".into(),
        ));
    }
    let owner = svc.clone();
    let target = tab.to_owned();
    let admitted = authority.clone();
    let selection = expected.clone();
    let original_generation = expected_nonce.clone();
    let observation = crate::service::provider_accounts::provisioning::start(svc, move || {
        #[cfg(any(test, feature = "test-support"))]
        expected_launch_commit(&owner, "account_provision");
        if admitted.is_some() || original_generation.is_some() {
            if let Some(authority) = &admitted { authority.validate(&owner)?; }
            if !owner.mux.read().get_tab(&target).is_some_and(|current| account_authority_matches(&selection, current)) { return Err("Runtime launch generation superseded".into()); }
            let provider = selection.agent_type.as_deref().ok_or("Account tab has no harness")?;
            let account = crate::service::provider_accounts::selected_account(&selection);
            let namespace = crate::service::provider_accounts::credential_namespace(&owner, provider, Some(account), selection.restorable_launch.account_namespace.as_deref())?;
            let invocation = crate::service::provider_accounts::invocation_at(&owner, provider, Some(account), namespace.as_deref())?;
            if let Some(authority) = &admitted { authority.validate(&owner)?; }
            if !owner.mux.read().get_tab(&target).is_some_and(|current| account_authority_matches(&selection, current)) { return Err("Runtime launch generation superseded".into()); }
            return Ok(invocation);
        }
        crate::service::provider_accounts::for_tab(&owner, &target)
    }).map_err(RuntimeFailure::Refused)?;
    let invocation = observation.wait(std::time::Duration::from_secs(60)).await;
    check_expected_nonce(svc, tab, expected_nonce.as_deref())?;
    let invocation = invocation.map_err(RuntimeFailure::Refused)?;
    if crate::crash::shutdown::is_shutting_down()
        || !svc.mux.read().get_tab(tab).is_some_and(|current| account_authority_matches(&expected, current)) {
        return Err(RuntimeFailure::Superseded);
    }
    let original = expected_nonce.as_ref().map(|_| expected.clone());
    codeterm_plugin::provider_account::scope(invocation, ensure_scoped(svc, tab, fork, expected_nonce, original)).await
    }).await
}

pub fn reserve_switch_launch(svc: &ServiceContext, authority: &Arc<crate::service::messenger::switch_handoff::SwitchLaunchAuthority>) -> Result<String, String> {
    authority.validate(svc)?;
    let (nonce, expected) = svc.reserve_agent_launch_binding_nonce_if_current(&authority.snapshot(), None, None)?;
    authority.reserved(expected);
    nonce.ok_or_else(|| "provider_switch_superseded: target unexpectedly authenticated before launch".into())
}

pub fn launch_switch_target(svc: &ServiceContext, authority: &Arc<crate::service::messenger::switch_handoff::SwitchLaunchAuthority>, config: Value) -> Result<(), String> {
    let expected = authority.snapshot();
    let provider = expected.agent_type.as_deref().ok_or("Runtime provider absent")?;
    let nonce = expected.session_binding_nonce.as_deref().ok_or("Runtime binding absent")?;
    {
        let mut mux = svc.mux.write();
        let current = mux.get_tab_mut(&expected.id).ok_or("Runtime tab absent")?;
        if !authority.matches(current) { return Err("provider_switch_superseded: target runtime launch changed".into()); }
        current.agent_runtime_config = Some(config);
        current.chat_view = true;
    }
    super::super::authority::record_spawn(svc, &expected.id, provider, nonce)?;
    crate::async_runtime::block_on(ensure(svc, &expected.id, None))
        .or_else(|error| super::login_pending_session(svc, &expected.id, error))
        .map(|_| ()).map_err(|error| format!("Runtime tab {} degraded: {error}", expected.id))
}

pub(super) fn account_authority_matches(expected: &crate::multiplexer::types::Tab, current: &crate::multiplexer::types::Tab) -> bool {
    current.agent_type == expected.agent_type && current.agent_session_id == expected.agent_session_id
        && current.session_binding_nonce == expected.session_binding_nonce
        && current.lifecycle_epoch == expected.lifecycle_epoch && current.lifecycle_operation_id == expected.lifecycle_operation_id
        && current.restorable_launch.account_id == expected.restorable_launch.account_id
        && current.restorable_launch.account_namespace == expected.restorable_launch.account_namespace
        && current.team_id == expected.team_id && current.parent_tab_id == expected.parent_tab_id
        && current.task_binding == expected.task_binding && current.role == expected.role && current.role_profile == expected.role_profile
        && current.report_target_tab_id == expected.report_target_tab_id && current.report_target_mesh_peer == expected.report_target_mesh_peer
        && current.spawned_by_tab_id == expected.spawned_by_tab_id && current.depth == expected.depth && current.can_spawn == expected.can_spawn
        && current.neighbor_authority == expected.neighbor_authority && current.ownership == expected.ownership
        && current.backing == expected.backing && current.cwd == expected.cwd && !current.frozen
}

pub async fn ensure_with_invocation(
    svc: &ServiceContext,
    tab: &str,
    invocation: Option<Arc<codeterm_plugin::provider_account::InvocationContext>>,
) -> Result<Arc<Session>, RuntimeFailure> {
    let authority = crate::service::messenger::switch_handoff::launch_authority(svc, tab);
    crate::service::messenger::switch_handoff::scope_launch(authority,
        codeterm_plugin::provider_account::scope(invocation, ensure_scoped(svc, tab, None, None, None))).await
}

pub(crate) fn account_boundary_ready(session: &Session, completion: Option<(&str, &str)>) -> bool {
    let context = session.context.lock();
    context.active_turn_id.is_none()
        && context.active_turns.is_empty()
        && context.pending_request.is_none()
        && session.turn_requests.lock().is_empty()
        && completion.is_none_or(|(thread, turn)| session.completions.lock().contains(thread, turn))
}

pub(crate) fn account_trial_idle(svc: &ServiceContext, session: &Session) -> bool {
    validate_binding(svc, session).is_ok()
        && session
            .thread_loaded
            .load(std::sync::atomic::Ordering::Acquire)
        && account_boundary_ready(session, None)
        && session.prompts.lock().is_empty()
}
