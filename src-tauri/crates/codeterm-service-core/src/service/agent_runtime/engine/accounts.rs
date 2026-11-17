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
    let invocation = codeterm_plugin::js_runtime::spawn_blocking(move || {
        crate::service::provider_accounts::for_tab(&owner, &target)
    })
    .await
    .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))?
    .map_err(RuntimeFailure::Refused)?;
    codeterm_plugin::provider_account::scope(invocation, ensure_scoped(svc, tab, fork)).await
}
