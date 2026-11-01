use super::*;
use codeterm_service_core::service::tab_mutation_authority::{TabMutationCaller, TabMutationScope};

pub(super) fn authorize(
    state: &AppState,
    headers: &HeaderMap,
    target: &str,
    scope: TabMutationScope<'_>,
) -> Result<(), (StatusCode, String)> {
    authorize_with_caller(state, headers, target, scope).map(|_| ())
}

pub(super) fn authorize_with_caller(
    state: &AppState,
    headers: &HeaderMap,
    target: &str,
    scope: TabMutationScope<'_>,
) -> Result<Option<codeterm_service_core::service::tab_credentials::TabCaller>, (StatusCode, String)>
{
    let caller = crate::auth::tab_caller(state, headers)?;
    state
        .svc
        .authorize_tab_mutation(
            caller
                .as_ref()
                .map_or(TabMutationCaller::Host, TabMutationCaller::Tab),
            target,
            scope,
        )
        .map_err(|error| (StatusCode::FORBIDDEN, error))?;
    Ok(caller)
}
