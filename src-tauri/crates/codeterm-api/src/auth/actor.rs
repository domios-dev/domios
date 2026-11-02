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
    match (origin, identity) {
        (AuthenticatedOrigin::Owner, CallerIdentity::Local | CallerIdentity::Peer { .. }) => {
            Ok(None)
        }
        (AuthenticatedOrigin::Agent, CallerIdentity::Tab(caller)) => Ok(Some(caller.clone())),
        _ => Err((
            StatusCode::FORBIDDEN,
            "This operation requires an authenticated local actor or owner credential".into(),
        )),
    }
}

#[derive(Clone)]
pub struct ActorHeaders(HeaderMap);

impl ActorHeaders {
    pub(crate) fn authenticate(
        state: &crate::routes::AppState,
        headers: HeaderMap,
    ) -> Result<Self, (StatusCode, String)> {
        let (identity, origin) = super::authenticated_request(state, &headers)?;
        Self::authenticated(&identity, origin, headers)
    }
    pub(crate) fn authenticated(
        identity: &CallerIdentity,
        origin: AuthenticatedOrigin,
        mut headers: HeaderMap,
    ) -> Result<Self, (StatusCode, String)> {
        if let Some(caller) = resolve(identity, origin)? {
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
        }
        Ok(Self(headers))
    }
}

impl Deref for ActorHeaders {
    type Target = HeaderMap;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for ActorHeaders {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let origin = parts.extensions.get::<AuthenticatedOrigin>().copied();
        let identity = parts.extensions.get::<CallerIdentity>();
        let (Some(origin), Some(identity)) = (origin, identity) else {
            return Err((
                StatusCode::FORBIDDEN,
                "Authenticated request authority required".into(),
            ));
        };
        Self::authenticated(identity, origin, parts.headers.clone())
    }
}

#[cfg(test)]
impl ActorHeaders {
    pub(crate) fn for_owner_test(headers: HeaderMap) -> Self {
        Self::authenticated(&CallerIdentity::Local, AuthenticatedOrigin::Owner, headers).unwrap()
    }
}

#[cfg(test)]
mod tests;
