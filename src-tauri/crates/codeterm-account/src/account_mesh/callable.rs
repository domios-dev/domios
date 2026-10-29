use super::callable_budget::retry_after;
pub use super::callable_budget::Clock as CallableClock;
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
    Retired,
    Status {
        status: u16,
        retry_after_seconds: Option<u64>,
        body: Option<Value>,
    },
}

impl CallableFailure {
    pub fn retry_after_seconds(&self) -> Option<u64> {
        match self {
            Self::Status {
                retry_after_seconds,
                body,
                ..
            } => {
                let structured = body
                    .as_ref()
                    .and_then(|body| body.pointer("/error/details/retry_after_seconds"))
                    .and_then(Value::as_u64);
                match (*retry_after_seconds, structured) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                }
            }
            Self::Offline | Self::Retired => None,
        }
    }
}

#[async_trait]
pub trait CallableGuard: Send + Sync {
    async fn current(&self) -> bool;
    async fn retired(&self) {
        std::future::pending::<()>().await;
    }
}

#[async_trait]
pub trait CallableSessionValidator: Send + Sync {
    async fn current(&self, expected: &Session) -> bool;
    async fn retired(&self, _expected: &Session) {
        std::future::pending::<()>().await;
    }
}

struct RegistryGuard<'a> {
    validator: Option<&'a dyn CallableSessionValidator>,
    session: &'a Session,
    captured_cleanup: bool,
}

#[async_trait]
impl CallableGuard for RegistryGuard<'_> {
    async fn retired(&self) {
        if !self.captured_cleanup {
            if let Some(validator) = self.validator {
                return validator.retired(self.session).await;
            }
        }
        std::future::pending::<()>().await;
    }

    async fn current(&self) -> bool {
        if self.captured_cleanup {
            return true;
        }
        match self.validator {
            Some(validator) => validator.current(self.session).await,
            None => true,
        }
    }
}

/// One Firebase callable POST: `{"data": ..}` in, `{"result": ..}` or `{"error": ..}` out.
#[async_trait]
pub trait CallableTransport: Send + Sync {
    fn retry_clock(&self) -> &dyn CallableClock {
        &super::callable_budget::RuntimeClock
    }
    async fn call_for_account(
        &self,
        account: &str,
        route: &str,
        id_token: &str,
        data: Value,
        guard: &dyn CallableGuard,
    ) -> Result<Value, CallableFailure> {
        super::callable_budget::run(
            self,
            account,
            route,
            id_token,
            data,
            guard,
            self.retry_clock(),
        )
        .await
    }

    async fn call(
        &self,
        route: &str,
        id_token: &str,
        data: Value,
    ) -> Result<Value, CallableFailure>;
}

pub fn refusal(failure: &CallableFailure) -> RegistryRefusal {
    let (status, body) = match failure {
        CallableFailure::Offline => return RegistryRefusal::Offline,
        CallableFailure::Retired => return RegistryRefusal::Cancelled,
        CallableFailure::Status { status, body, .. } => (*status, body),
    };
    if body.as_ref().is_some_and(is_session_revoked) {
        return RegistryRefusal::Unauthenticated;
    }
    match status {
        404 => return RegistryRefusal::Unavailable,
        403 => {
            let typed = body
                .as_ref()
                .and_then(|body| body.pointer("/error/details"))
                .map(RegistryRefusal::from_details);
            return typed
                .filter(|reason| {
                    !matches!(
                        reason,
                        RegistryRefusal::Unavailable
                            | RegistryRefusal::Offline
                            | RegistryRefusal::AuthorityUnavailable
                            | RegistryRefusal::Unauthenticated
                            | RegistryRefusal::RateLimited { .. }
                    )
                })
                .unwrap_or(RegistryRefusal::Unavailable);
        }
        429 => {
            if let Some(reason @ RegistryRefusal::EnrollmentBudget { .. }) = body
                .as_ref()
                .and_then(|body| body.pointer("/error/details"))
                .map(RegistryRefusal::from_details)
            {
                return reason;
            }
            return RegistryRefusal::RateLimited {
                retry_after_seconds: failure.retry_after_seconds().unwrap_or(0).max(1),
            };
        }
        401 => {
            let typed = body
                .as_ref()
                .and_then(|body| body.pointer("/error/details"))
                .map(RegistryRefusal::from_details);
            return typed
                .filter(|reason| {
                    matches!(
                        reason,
                        RegistryRefusal::ReauthRequired | RegistryRefusal::AppCheck
                    )
                })
                .unwrap_or(RegistryRefusal::Unauthenticated);
        }
        500..=599 => {
            return if body
                .as_ref()
                .and_then(|body| body.pointer("/error/details"))
                .map(RegistryRefusal::from_details)
                == Some(RegistryRefusal::AuthorityUnavailable)
            {
                RegistryRefusal::AuthorityUnavailable
            } else {
                RegistryRefusal::Unavailable
            };
        }
        _ => {}
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
    validator: Option<Arc<dyn CallableSessionValidator>>,
}

impl<T: CallableTransport> CallableRegistry<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            revocations: None,
            validator: None,
        }
    }

    pub fn with_validator(mut self, validator: Arc<dyn CallableSessionValidator>) -> Self {
        self.validator = Some(validator);
        self
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
        self.call_guarded(route, session, data, false).await
    }

    async fn call_guarded<R: DeserializeOwned>(
        &self,
        route: &str,
        session: &Session,
        data: &impl Serialize,
        captured_cleanup: bool,
    ) -> RegistryResult<R> {
        let guard = RegistryGuard {
            validator: self.validator.as_deref(),
            session,
            captured_cleanup,
        };
        let data = serde_json::to_value(data).map_err(|_| RegistryRefusal::Invalid)?;
        let response = self
            .transport
            .call_for_account(
                &session.identity.uid,
                route,
                session.id_token.expose(),
                data,
                &guard,
            )
            .await;
        if let Err(failure) = &response {
            let status = match failure {
                CallableFailure::Offline | CallableFailure::Retired => None,
                CallableFailure::Status { status, .. } => Some(*status),
            };
            log::debug!(
                "Account mesh callable failed: route={route} status={status:?} kind={:?}",
                refusal(failure)
            );
        }
        let body = match response {
            Ok(body) if !is_session_revoked(&body) => body,
            response => {
                let body = match &response {
                    Ok(body) => Some(body),
                    Err(CallableFailure::Status { body, .. }) => body.as_ref(),
                    Err(CallableFailure::Offline | CallableFailure::Retired) => None,
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
        result(body).inspect_err(|refusal| {
            log::debug!("Account mesh callable result failed: route={route} kind={refusal:?}");
        })
    }
}

#[async_trait]
impl<T: CallableTransport> MeshRegistry for CallableRegistry<T> {
    fn handles_call_deadline(&self) -> bool {
        true
    }
    async fn revoke_prior_sessions(
        &self,
        session: &Session,
        target: &PriorSessionsTarget,
    ) -> RegistryResult<PriorSessionsRevokedResult> {
        self.call_guarded("revokePriorInstallationSessions", session, target, true)
            .await
    }
    async fn challenge(
        &self,
        session: &Session,
        request: &ChallengeRequest,
    ) -> RegistryResult<String> {
        if request.endpoint == ProofEndpoint::GetEntitlements {
            let installation_id = request
                .body
                .get("installation_id")
                .ok_or(RegistryRefusal::Invalid)?;
            return self
                .call::<ChallengeResult>(
                    "issueInstallationChallenge",
                    session,
                    &serde_json::json!({
                        "installation_id": installation_id,
                        "endpoint": request.endpoint,
                        "key_id": request.key_id,
                        "body": request.body,
                    }),
                )
                .await
                .map(|result| result.nonce);
        }
        self.call_guarded::<ChallengeResult>(
            ROUTE_CHALLENGE,
            session,
            request,
            cleanup_challenge(request),
        )
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
        self.call_guarded(
            ROUTE_ADMISSION,
            session,
            envelope,
            cleanup_operation(&envelope.request),
        )
        .await
    }
}

pub(super) fn cleanup_operation(request: &MeshAdmissionBody) -> bool {
    matches!(
        &request.operation,
        MeshOperation::Withdraw { .. }
            | MeshOperation::Presence {
                presence: RegistryPresence::SignedOut,
                ..
            }
    ) || matches!(&request.operation, MeshOperation::SessionRevoke { target } if target.installation_id == request.installation_id)
}

pub(super) fn cleanup_challenge(request: &ChallengeRequest) -> bool {
    if request.endpoint != ProofEndpoint::MeshAdmission {
        return false;
    }
    match request.body.get("operation").and_then(Value::as_str) {
        Some("withdraw") => true,
        Some("presence") => {
            request.body.get("presence").and_then(Value::as_str) == Some("signed_out")
        }
        Some("session_revoke") => request
            .body
            .get("installation_id")
            .and_then(Value::as_str)
            .is_some_and(|installation| {
                request
                    .body
                    .pointer("/target/installation_id")
                    .and_then(Value::as_str)
                    == Some(installation)
            }),
        _ => false,
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
            retry_after_seconds: None,
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
        let retry_after_seconds = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| retry_after(value, std::time::SystemTime::now()));
        let body = response.json::<Value>().await.ok();
        if status.is_success() {
            body.ok_or(CallableFailure::Status {
                status: status.as_u16(),
                retry_after_seconds,
                body: None,
            })
        } else {
            Err(CallableFailure::Status {
                status: status.as_u16(),
                retry_after_seconds,
                body,
            })
        }
    }
}
