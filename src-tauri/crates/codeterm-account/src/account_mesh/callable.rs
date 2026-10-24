use super::ports::{MeshRegistry, RegistryResult};
use super::wire::*;
use crate::{
    session_revocation::{is_session_revoked, SessionRevocationObserver},
    Session,
};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallableFailure {
    /// No HTTP response at all: network, DNS, TLS or timeout.
    Offline,
    Status {
        status: u16,
        body: Option<Value>,
    },
}

/// One Firebase callable POST: `{"data": ..}` in, `{"result": ..}` or `{"error": ..}` out.
#[async_trait]
pub trait CallableTransport: Send + Sync {
    async fn call(
        &self,
        route: &str,
        id_token: &str,
        data: Value,
    ) -> Result<Value, CallableFailure>;
}

/// Server refusal from a callable error: typed `details` first, then the HTTP status class.
pub fn refusal(failure: &CallableFailure) -> RegistryRefusal {
    let (status, body) = match failure {
        CallableFailure::Offline => return RegistryRefusal::Offline,
        CallableFailure::Status { status, body } => (*status, body),
    };
    if body.as_ref().is_some_and(is_session_revoked) {
        return RegistryRefusal::Unauthenticated;
    }
    if let Some(details) = body
        .as_ref()
        .and_then(|body| body.pointer("/error/details"))
    {
        return RegistryRefusal::from_details(details);
    }
    match status {
        400 => RegistryRefusal::Invalid,
        401 => RegistryRefusal::Unauthenticated,
        403 => RegistryRefusal::Forbidden,
        409 => RegistryRefusal::Conflict,
        _ => RegistryRefusal::Unavailable,
    }
}

/// A malformed success body is an outage, never a partial grant.
pub fn result<T: DeserializeOwned>(body: Value) -> RegistryResult<T> {
    let Value::Object(mut body) = body else {
        return Err(RegistryRefusal::Unavailable);
    };
    let result = body.remove("result").ok_or(RegistryRefusal::Unavailable)?;
    serde_json::from_value(result).map_err(|_| RegistryRefusal::Unavailable)
}

pub struct CallableRegistry<T> {
    transport: T,
    revocations: Option<Arc<dyn SessionRevocationObserver>>,
}

impl<T: CallableTransport> CallableRegistry<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            revocations: None,
        }
    }

    pub fn with_revocations(mut self, observer: Arc<dyn SessionRevocationObserver>) -> Self {
        self.revocations = Some(observer);
        self
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    async fn call<R: DeserializeOwned>(
        &self,
        route: &str,
        session: &Session,
        data: &impl Serialize,
    ) -> RegistryResult<R> {
        let data = serde_json::to_value(data).map_err(|_| RegistryRefusal::Invalid)?;
        let response = self
            .transport
            .call(route, session.id_token.expose(), data)
            .await;
        let body = match response {
            Ok(body) if !is_session_revoked(&body) => body,
            response => {
                let body = match &response {
                    Ok(body) => Some(body),
                    Err(CallableFailure::Status { body, .. }) => body.as_ref(),
                    Err(CallableFailure::Offline) => None,
                };
                if body.is_some_and(is_session_revoked) {
                    if let Some(observer) = &self.revocations {
                        observer.revoked(session).await;
                    }
                    return Err(RegistryRefusal::Unauthenticated);
                }
                return Err(response
                    .as_ref()
                    .err()
                    .map(refusal)
                    .unwrap_or(RegistryRefusal::Unavailable));
            }
        };
        result(body)
    }
}

#[async_trait]
impl<T: CallableTransport> MeshRegistry for CallableRegistry<T> {
    async fn challenge(
        &self,
        session: &Session,
        request: &ChallengeRequest,
    ) -> RegistryResult<String> {
        self.call::<ChallengeResult>(ROUTE_CHALLENGE, session, request)
            .await
            .map(|result| result.nonce)
    }
    async fn enroll_key(
        &self,
        session: &Session,
        request: &InstallationEnrollment,
    ) -> RegistryResult<KeyEnrollment> {
        self.call(ROUTE_ENROLL_KEY, session, request).await
    }
    async fn admission(
        &self,
        session: &Session,
        envelope: &SignedEnvelope<MeshAdmissionBody>,
    ) -> RegistryResult<MeshResult> {
        self.call(ROUTE_ADMISSION, session, envelope).await
    }
}

/// HTTPS callable transport; no redirects, bounded time, and no request or account logging.
pub struct HttpCallable {
    base: url::Url,
    client: reqwest::Client,
}

impl HttpCallable {
    pub fn new(base: &str) -> crate::Result<Self> {
        let base = url::Url::parse(base).map_err(|_| crate::Error::Configuration)?;
        if base.scheme() != "https"
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || !base.path().ends_with('/')
        {
            return Err(crate::Error::Configuration);
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| crate::Error::Transport)?;
        Ok(Self { base, client })
    }
}

#[async_trait]
impl CallableTransport for HttpCallable {
    async fn call(
        &self,
        route: &str,
        id_token: &str,
        data: Value,
    ) -> Result<Value, CallableFailure> {
        let url = self.base.join(route).map_err(|_| CallableFailure::Status {
            status: 400,
            body: None,
        })?;
        let response = self
            .client
            .post(url)
            .bearer_auth(id_token)
            .json(&serde_json::json!({ "data": data }))
            .send()
            .await
            .map_err(|_| CallableFailure::Offline)?;
        let status = response.status();
        let body = response.json::<Value>().await.ok();
        if status.is_success() {
            body.ok_or(CallableFailure::Status {
                status: status.as_u16(),
                body: None,
            })
        } else {
            Err(CallableFailure::Status {
                status: status.as_u16(),
                body,
            })
        }
    }
}
