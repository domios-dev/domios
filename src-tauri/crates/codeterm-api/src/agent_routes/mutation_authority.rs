use super::*;
use codeterm_service_core::service::tab_mutation_authority::{TabMutationCaller, TabMutationScope};

pub(super) fn authorize(
    state: &AppState,
    headers: &HeaderMap,
    target: &str,
    scope: TabMutationScope<'_>,
) -> Result<(), (StatusCode, String)> {
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
        .map_err(|error| (StatusCode::FORBIDDEN, error))
}
