use super::*;

pub(super) async fn ensure(
    svc: &ServiceContext,
    tab: &str,
    fork: Option<(String, String)>,
) -> Result<Arc<Session>, RuntimeFailure> {
    if svc
        .mux
        .read()
        .get_tab(tab)
        .is_some_and(|target| target.frozen)
    {
        return Err(RuntimeFailure::Refused(
            "Tab is sleeping; open or address it to wake".into(),
        ));
    }
    let owner = svc.clone();
    let target = tab.to_owned();
    let invocation = await_account_supervision(codeterm_plugin::js_runtime::spawn_blocking(move || {
        crate::service::provider_accounts::for_tab(&owner, &target)
    }), std::time::Duration::from_secs(60))
    .await?
    .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))?
    .map_err(RuntimeFailure::Refused)?;
    codeterm_plugin::provider_account::scope(invocation, ensure_scoped(svc, tab, fork)).await
}

pub(super) async fn await_account_supervision<T>(
    provision: impl std::future::Future<Output = T>,
    deadline: std::time::Duration,
) -> Result<T, RuntimeFailure> {
    tokio::time::timeout(deadline, provision)
        .await
        .map_err(|_| RuntimeFailure::Deadline)
}

pub async fn ensure_with_invocation(
    svc: &ServiceContext,
    tab: &str,
    invocation: Option<Arc<codeterm_plugin::provider_account::InvocationContext>>,
) -> Result<Arc<Session>, RuntimeFailure> {
    codeterm_plugin::provider_account::scope(invocation, ensure_scoped(svc, tab, None)).await
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
