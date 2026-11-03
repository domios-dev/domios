use std::ops::Deref;

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, HeaderMap, StatusCode},
};
use codeterm_service_core::service::tab_credentials::TabCaller;

use super::{AuthenticatedOrigin, CallerIdentity};

pub(super) fn resolve(
    identity: &CallerIdentity,
    origin: AuthenticatedOrigin,
) -> Result<Option<TabCaller>, (StatusCode, String)> {
    let actor = RequestActor::resolve(identity, origin, None)?;
    match actor {
        RequestActor::Owner => Ok(None),
        RequestActor::LocalAgent(caller) => Ok(Some(caller)),
        _ => Err((
            StatusCode::FORBIDDEN,
            "This operation requires an authenticated local actor or owner credential".into(),
        )),
    }
}

#[derive(Clone)]
pub struct RequestHeaders<const TARGET_LOCAL: bool> {
    headers: HeaderMap,
    pub(crate) actor: RequestActor,
}

pub type ActorHeaders = RequestHeaders<true>;
pub type AgentHeaders = RequestHeaders<false>;

#[derive(Clone, Debug)]
pub enum RequestActor {
    Owner,
    LocalAgent(TabCaller),
    RemoteAgent(codeterm_service_core::mesh::crown::AuthenticatedAgent),
    Peer,
    Service,
}

impl RequestActor {
    pub(crate) fn authenticate(
        state: &crate::routes::AppState,
        headers: &HeaderMap,
    ) -> Result<Self, (StatusCode, String)> {
        let (identity, origin) = super::authenticated_request(state, headers)?;
        Self::resolve(
            &identity,
            origin,
            super::authenticated_agent(state, headers)?,
        )
    }

    fn resolve(
        identity: &CallerIdentity,
        origin: AuthenticatedOrigin,
        agent: Option<codeterm_service_core::mesh::crown::AuthenticatedAgent>,
    ) -> Result<Self, (StatusCode, String)> {
        match (origin, identity) {
            (AuthenticatedOrigin::Owner, _) => Ok(Self::Owner),
            (AuthenticatedOrigin::Agent, CallerIdentity::Tab(caller)) => {
                Ok(Self::LocalAgent(caller.clone()))
            }
            (AuthenticatedOrigin::Agent, _) => agent.map(Self::RemoteAgent).ok_or((
                StatusCode::FORBIDDEN,
                "Signed remote agent identity required".into(),
            )),
            (AuthenticatedOrigin::Mesh, _) => Ok(Self::Peer),
            (AuthenticatedOrigin::Service, _) => Ok(Self::Service),
        }
    }

    fn accepts(&self, target_local: bool) -> bool {
        matches!(self, Self::Owner | Self::LocalAgent(_))
            || (!target_local && matches!(self, Self::RemoteAgent(_) | Self::Peer))
    }

    pub(crate) fn local_tab(&self) -> Option<&str> {
        match self {
            Self::LocalAgent(caller) => Some(&caller.tab_id),
            _ => None,
        }
    }
}

impl<const TARGET_LOCAL: bool> RequestHeaders<TARGET_LOCAL> {
    pub(crate) fn authenticate(
        state: &crate::routes::AppState,
        headers: HeaderMap,
    ) -> Result<Self, (StatusCode, String)> {
        let actor = RequestActor::authenticate(state, &headers)?;
        Self::from_actor(actor, headers)
    }

    pub(crate) fn authenticated(
        identity: &CallerIdentity,
        origin: AuthenticatedOrigin,
        headers: HeaderMap,
    ) -> Result<Self, (StatusCode, String)> {
        Self::from_actor(RequestActor::resolve(identity, origin, None)?, headers)
    }

    fn from_actor(
        actor: RequestActor,
        mut headers: HeaderMap,
    ) -> Result<Self, (StatusCode, String)> {
        if !actor.accepts(TARGET_LOCAL) {
            return Err((
                StatusCode::FORBIDDEN,
                "This operation requires an authenticated local actor or owner credential".into(),
            ));
        }
        if let RequestActor::LocalAgent(caller) = &actor {
            caller
                .validate_claim(
                    headers
                        .get("x-codeterm-tab-id")
                        .and_then(|value| value.to_str().ok()),
                )
                .map_err(|error| (StatusCode::FORBIDDEN, error))?;
            headers.insert(
                "x-codeterm-tab-id",
                caller
                    .tab_id
                    .parse()
                    .map_err(|_| (StatusCode::FORBIDDEN, "Invalid authenticated actor".into()))?,
            );
        } else if !matches!(actor, RequestActor::Owner) {
            headers.remove("x-codeterm-tab-id");
            headers.remove("x-codeterm-session-id");
            headers.remove("x-codeterm-binding-nonce");
        }
        Ok(Self { headers, actor })
    }

    pub(crate) fn local_tab_id(&self) -> Option<String> {
        match &self.actor {
            RequestActor::Owner => self
                .headers
                .get("x-codeterm-tab-id")
                .and_then(|value| value.to_str().ok())
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            actor => actor.local_tab().map(str::to_owned),
        }
    }
}

impl AgentHeaders {
    pub(crate) fn save_memory(
        &self,
        state: &crate::routes::AppState,
        item: codeterm_service_core::mem::NewMemItem,
        replace: bool,
    ) -> Result<codeterm_mem::manager_crud::SaveOutcome, String> {
        if matches!(
            self.actor,
            RequestActor::Owner | RequestActor::LocalAgent(_)
        ) {
            let local = ActorHeaders::from_actor(self.actor.clone(), self.headers.clone())
                .map_err(|(_, error)| error)?;
            state.svc.save_memory_for_caller(
                &crate::tab_routes::authenticated_plan_caller(&local),
                item,
                replace,
            )
        } else {
            state.svc.mem.save_with_outcome(item, replace)
        }
    }
}

impl<const TARGET_LOCAL: bool> Deref for RequestHeaders<TARGET_LOCAL> {
    type Target = HeaderMap;
    fn deref(&self) -> &Self::Target {
        &self.headers
    }
}

impl<const TARGET_LOCAL: bool> FromRequestParts<crate::routes::AppState>
    for RequestHeaders<TARGET_LOCAL>
{
    type Rejection = (StatusCode, String);
    async fn from_request_parts(
        parts: &mut Parts,
        state: &crate::routes::AppState,
    ) -> Result<Self, Self::Rejection> {
        Self::authenticate(state, parts.headers.clone())
    }
}

#[cfg(test)]
impl<const TARGET_LOCAL: bool> RequestHeaders<TARGET_LOCAL> {
    pub(crate) fn for_owner_test(headers: HeaderMap) -> Self {
        Self::authenticated(&CallerIdentity::Local, AuthenticatedOrigin::Owner, headers).unwrap()
    }
}

#[cfg(test)]
mod tests;
