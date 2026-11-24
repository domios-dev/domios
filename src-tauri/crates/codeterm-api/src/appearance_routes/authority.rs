use axum::http::{HeaderMap, StatusCode};
use codeterm_service_core::mesh::types::AccessLevel;

use super::Refusal;
use crate::{
    auth::{self, AuthenticatedOrigin, CallerIdentity},
    routes::AppState,
};

const MAIN_OR_OWNER_REQUIRED: &str =
    "Appearance proposals require the owner or authenticated General Main on a paired machine in the same account.";
const OWNER_REQUIRED: &str =
    "Owner authorization required to apply or undo an appearance proposal.";
pub(crate) const FULL_ACCESS_REQUIRED: &str =
    "Appearance requests require Full mesh access on the destination. The owner can change this peer's access in Mesh settings there.";

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Inspect,
    Store,
    Commit,
}

enum Caller {
    LocalOwner,
    LocalMain,
    AccountPeer {
        origin: AuthenticatedOrigin,
        main: bool,
        access: AccessLevel,
    },
    Other,
}

fn authorize(caller: Caller, operation: Operation) -> Result<(), &'static str> {
    match (caller, operation) {
        (Caller::AccountPeer { access, .. }, _) if access != AccessLevel::Full => {
            Err(FULL_ACCESS_REQUIRED)
        }
        (
            Caller::LocalOwner
            | Caller::AccountPeer {
                origin: AuthenticatedOrigin::Owner,
                ..
            },
            _,
        )
        | (Caller::LocalMain, Operation::Inspect | Operation::Store)
        | (
            Caller::AccountPeer {
                origin: AuthenticatedOrigin::Agent,
                main: true,
                ..
            },
            Operation::Store,
        ) => Ok(()),
        (_, Operation::Commit) => Err(OWNER_REQUIRED),
        _ => Err(MAIN_OR_OWNER_REQUIRED),
    }
}

pub(super) fn authorize_request(
    state: &AppState,
    headers: &HeaderMap,
    operation: Operation,
) -> Result<(), Refusal> {
    let (identity, origin) = auth::authenticated_request(state, headers)?;
    let caller = match identity {
        CallerIdentity::Local if origin == AuthenticatedOrigin::Owner => Caller::LocalOwner,
        CallerIdentity::Tab(caller) if auth::is_bound_main(state, &caller) => Caller::LocalMain,
        CallerIdentity::Peer { access_level, .. } => Caller::AccountPeer {
            origin,
            main: matches!(operation, Operation::Store)
                && auth::authenticated_agent_for_origin(state, headers, origin)?
                    .is_some_and(|agent| agent.general_main),
            access: access_level,
        },
        _ => Caller::Other,
    };
    authorize(caller, operation).map_err(|message| (StatusCode::FORBIDDEN, message.into()))
}

pub(crate) fn is_appearance_path(path: &str) -> bool {
    codeterm_service_core::api_support::access_policy::request_path(path)
        .starts_with("/api/appearance/")
}

pub(crate) fn is_proposal_path(method: &str, path: &str) -> bool {
    method.eq_ignore_ascii_case("POST")
        && codeterm_service_core::api_support::access_policy::request_path(path)
            .strip_prefix("/api/appearance/")
            .is_some_and(|machine| !machine.is_empty() && !machine.contains('/'))
}

#[cfg(test)]
mod tests;
