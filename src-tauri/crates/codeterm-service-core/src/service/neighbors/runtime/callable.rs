use crate::service::ServiceContext;
use async_trait::async_trait;
use codeterm_account::account_mesh::callable::{CallableFailure, CallableTransport, HttpCallable};
use codeterm_neighbors::{adapter::AuthenticatedCall, Error, Result};
use serde_json::Value;
use zeroize::Zeroizing;

pub(in crate::service::neighbors) struct Calls {
    pub service: ServiceContext,
    pub uid: String,
    pub generation: u64,
}

#[derive(Default)]
struct AuthFailures(parking_lot::Mutex<std::collections::HashSet<(String, u64)>>);

impl Calls {
    async fn local_session(&self) -> Result<codeterm_account::Session> {
        let account = self
            .service
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .cloned()
            .ok_or(Error::Revoked)?;
        let session = account.session().await.ok_or(Error::Revoked)?;
        if session.identity.uid != self.uid || session.login_generation != self.generation {
            return Err(Error::Revoked);
        }
        Ok(session)
    }
    async fn session(&self) -> Result<codeterm_account::Session> {
        let account = self.service.account().await.map_err(|_| Error::Transport)?;
        let session = account
            .current_session()
            .await
            .map_err(|error| {
                log::debug!("[neighbor-listen] operation=auth.session kind={error:?}");
                Error::Transport
            })?
            .ok_or(Error::Revoked)?;
        if session.identity.uid != self.uid || session.login_generation != self.generation {
            return Err(Error::Revoked);
        }
        Ok(session)
    }

    async fn validate_session(&self, expected: &codeterm_account::Session) -> Result<()> {
        let current = self.local_session().await?;
        if super::super::same_session(Some(&current), expected) {
            Ok(())
        } else {
            Err(Error::Revoked)
        }
    }
}

#[async_trait]
impl AuthenticatedCall for Calls {
    fn require_paid(&self) -> Result<()> {
        self.service.require_neighbor_paid(&self.uid)
    }
    fn firestore_failure(
        &self,
        operation: &'static str,
        target: &str,
        code: Option<i32>,
        kind: &str,
    ) {
        if code == Some(16) {
            self.service
                .extensions
                .get_or_init(AuthFailures::default)
                .0
                .lock()
                .insert((self.uid.clone(), self.generation));
        }
        log::debug!(
            "[neighbor-listen] operation={operation} target={target} code={code:?} kind={kind}"
        );
    }
    async fn id_token(&self) -> Result<Zeroizing<String>> {
        let mut session = self.session().await?;
        let failed = {
            let mut failures = self
                .service
                .extensions
                .get_or_init(AuthFailures::default)
                .0
                .lock();
            let failed = failures.remove(&(self.uid.clone(), self.generation));
            failures.retain(|key| key.0 == self.uid && key.1 == self.generation);
            failed
        };
        if failed {
            session = self.recover_session(&session).await?;
        }
        self.validate_session(&session).await?;
        Ok(Zeroizing::new(session.id_token.expose().to_owned()))
    }
    async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
        self.invoke_at(
            codeterm_account::firebase_endpoints::current().functions,
            route,
            data,
        )
        .await
    }
}

impl Calls {
    pub(super) async fn invoke_at(
        &self,
        functions: &str,
        route: &str,
        data: Value,
    ) -> Result<Value> {
        self.local_session()
            .await
            .map_err(|error| key_mutation_error(route, error))?;
        let result = self
            .service
            .neighbor_backend_read(
                route,
                &self.uid,
                self.generation,
                self.invoke_owned(functions, route, data),
            )
            .await;
        self.local_session()
            .await
            .map_err(|error| key_mutation_error(route, error))?;
        result
    }
    async fn invoke_owned(&self, functions: &str, route: &str, data: Value) -> Result<Value> {
        let transport = HttpCallable::new(functions).map_err(|_| Error::Transport)?;
        self.invoke_with_transport(&transport, route, data).await
    }

    async fn recover_session(
        &self,
        expected: &codeterm_account::Session,
    ) -> Result<codeterm_account::Session> {
        let account = self
            .service
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .ok_or(Error::Revoked)?;
        let session = match account.recover_session(expected).await {
            Ok(session) => session.ok_or(Error::Revoked)?,
            Err(error) => {
                return Err(
                    if error == codeterm_account::Error::Cancelled
                        || account.session().await.is_none()
                    {
                        Error::Revoked
                    } else {
                        Error::Transport
                    },
                )
            }
        };
        self.validate_session(&session).await?;
        Ok(session)
    }

    async fn invoke_with_transport(
        &self,
        transport: &dyn CallableTransport,
        route: &str,
        data: Value,
    ) -> Result<Value> {
        if paid_route(route) {
            self.require_paid()?;
        }
        if route == "friendsEnrollVerifierKey"
            && self
                .service
                .neighbor_key_inventory_pending(&self.uid, self.generation)
        {
            return Err(Error::KeyInventoryPending);
        }
        let mut session = self
            .session()
            .await
            .map_err(|error| key_mutation_error(route, error))?;
        let mut response = transport
            .call_for_account(
                &self.uid,
                route,
                session.id_token.expose(),
                data.clone(),
                &PaidCallGuard {
                    calls: self,
                    expected: &session,
                    paid: paid_route(route),
                },
            )
            .await;
        if matches!(&response, Err(CallableFailure::Status { status: 401, body, .. }) if recoverable_authentication(body.as_ref()))
        {
            session = self
                .recover_session(&session)
                .await
                .map_err(|error| key_mutation_error(route, error))?;
            if matches!(
                route,
                "friendsList" | "friendsReceiverStatus" | "friendsOwnKeys"
            ) {
                response = transport
                    .call_for_account(
                        &self.uid,
                        route,
                        session.id_token.expose(),
                        data,
                        &PaidCallGuard {
                            calls: self,
                            expected: &session,
                            paid: paid_route(route),
                        },
                    )
                    .await;
            } else {
                return Err(Error::Transport);
            }
        }
        let body = match response {
            Ok(body) => body,
            Err(CallableFailure::Status {
                status,
                body,
                retry_after_seconds,
            }) => {
                if body
                    .as_ref()
                    .is_some_and(codeterm_account::session_revocation::is_session_revoked)
                {
                    self.service.session_revocations().revoked(&session).await;
                    return Err(Error::SessionRevoked);
                }
                return Err(classify_callable_error(
                    status,
                    body.as_ref(),
                    retry_after_seconds,
                ));
            }
            Err(CallableFailure::Retired) => return Err(key_mutation_error(route, Error::Revoked)),
            Err(_) => return Err(Error::Transport),
        };
        let key_mutation = route == "friendsEnrollVerifierKey";
        self.validate_session(&session).await.map_err(|error| {
            if key_mutation {
                Error::KeyInventoryPending
            } else {
                error
            }
        })?;
        let result = body.get("result").cloned().ok_or(Error::Transport)?;
        if key_mutation {
            let key = result["verifier_key_id"]
                .as_str()
                .filter(|key| codeterm_neighbors::wire::key_id(key))
                .ok_or(Error::KeyInventoryPending)?;
            self.service
                .fence_neighbor_key_inventory(&self.uid, self.generation, key)?;
            self.service
                .invalidate_neighbor_list()
                .await
                .map_err(|_| Error::KeyInventoryPending)?;
            if self.service.neighbor_registry_activation_active()
                && self
                    .service
                    .neighbor_key_inventory_pending(&self.uid, self.generation)
            {
                return Err(Error::KeyInventoryPending);
            }
            let guard = PaidCallGuard {
                calls: self,
                expected: &session,
                paid: true,
            };
            use codeterm_account::account_mesh::callable::CallableGuard;
            tokio::select! {
                biased;
                _ = guard.retired() => return Err(Error::KeyInventoryPending),
                confirmation = self.service.wait_neighbor_key_inventory(&self.uid, self.generation, key) => confirmation?,
            }
            self.validate_session(&session)
                .await
                .map_err(|_| Error::KeyInventoryPending)?;
            return Ok(result);
        }
        if matches!(
            route,
            "friendsRevokeVerifierKey"
                | "friendsSetFriendReceiver"
                | "friendsSetPaused"
                | "friendsSetDailyLimit"
                | "friendsSetFileDailyLimit"
        ) {
            self.service.invalidate_neighbor_list().await?;
        }
        Ok(result)
    }
}

fn key_mutation_error(route: &str, error: Error) -> Error {
    if route == "friendsEnrollVerifierKey"
        && matches!(
            error,
            Error::Revoked | Error::SessionRevoked | Error::Disabled
        )
    {
        Error::KeyInventoryPending
    } else {
        error
    }
}

fn recoverable_authentication(body: Option<&Value>) -> bool {
    !body.is_some_and(codeterm_account::session_revocation::is_session_revoked)
        && body.is_none_or(|body| matches!(refusal(body), Error::Transport | Error::Refused))
}

#[cfg(test)]
fn callable_error(status: u16, body: Option<&Value>) -> Error {
    classify_callable_error(status, body, None)
}

fn classify_callable_error(status: u16, body: Option<&Value>, retry_after: Option<u64>) -> Error {
    let structured = body.map(refusal).unwrap_or(Error::Refused);
    if matches!(
        structured,
        Error::Locked | Error::Revoked | Error::SessionRevoked
    ) {
        return structured;
    }
    let seconds = body
        .and_then(|body| body.pointer("/error/details/retry_after_seconds"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .max(retry_after.unwrap_or(0));
    match status {
        404 => Error::Unavailable,
        403 => match structured {
            Error::Refused
            | Error::Transport
            | Error::RateLimited { .. }
            | Error::AggregateLimited { .. } => Error::PermissionDenied,
            error => error,
        },
        401 | 500..=599 => Error::Http {
            status,
            retry_after_seconds: seconds,
        },
        429 => match structured {
            Error::RateLimited { .. } => Error::RateLimited {
                retry_after_seconds: seconds,
            },
            Error::AggregateLimited { .. } => Error::AggregateLimited {
                retry_after_seconds: seconds,
            },
            _ => Error::Http {
                status,
                retry_after_seconds: seconds,
            },
        },
        _ => structured,
    }
}

fn refusal(body: &Value) -> Error {
    if let Some(failure) = codeterm_neighbors::files::refusal::Failure::from_callable(body) {
        return Error::File(failure);
    }
    match body
        .pointer("/error/details/reason")
        .and_then(Value::as_str)
        .or_else(|| body.pointer("/error/message").and_then(Value::as_str))
        .or_else(|| body.pointer("/error/status").and_then(Value::as_str))
    {
        Some(reason @ ("neighbor_daily_limit" | "aggregate_daily_limited")) => {
            match (
                body.pointer("/error/details/limit").and_then(Value::as_u64),
                body.pointer("/error/details/resets_at")
                    .and_then(Value::as_u64),
            ) {
                (Some(limit), Some(resets_at)) if reason == "neighbor_daily_limit" => {
                    Error::DailyLimit { limit, resets_at }
                }
                (Some(limit), Some(resets_at)) => Error::AggregateDailyLimited { limit, resets_at },
                _ => Error::Refused,
            }
        }
        Some("limit_out_of_range") => Error::LimitOutOfRange,
        Some("invalid_request") => Error::InvalidRequest,
        Some("friendship_not_found" | "not_friends") => Error::FriendshipNotFound,
        Some("invalid_envelope") => Error::InvalidEnvelope,
        Some("too_large") => Error::MessageTooLarge {
            limit: codeterm_neighbors::wire::MAX_PLAINTEXT_BYTES as u64,
        },
        Some("operation_id_conflict") => Error::OperationIdConflict,
        Some("installation_unverified") => Error::InstallationUnverified,
        Some("receiver_elsewhere") => Error::ReceiverElsewhere,
        Some("receiver_revision_stale") => Error::ReceiverRevisionStale,
        Some("not_receiver") => Error::NotReceiver,
        Some("recipient_not_receiving") => Error::RecipientNotReceiving,
        Some("recipient_paused") => Error::RecipientPaused,
        Some("recipient_locked") => Error::RecipientLocked,
        Some(reason @ ("rate_limited" | "aggregate_limited")) => {
            match body
                .pointer("/error/details/retry_after_seconds")
                .and_then(Value::as_u64)
            {
                Some(retry_after_seconds) if reason == "rate_limited" => Error::RateLimited {
                    retry_after_seconds,
                },
                Some(retry_after_seconds) => Error::AggregateLimited {
                    retry_after_seconds,
                },
                None => Error::Refused,
            }
        }
        Some("friends_locked" | "entitlement_locked") => Error::Locked,
        Some("permission_denied" | "PERMISSION_DENIED") => Error::PermissionDenied,
        Some("unauthenticated" | "UNAUTHENTICATED") => Error::Transport,
        Some("key_revoked" | "unknown_key") => Error::Revoked,
        Some("key_not_found") => Error::KeyNotFound,
        Some("invalid_key") => Error::InvalidKey,
        Some("entitlements_unavailable") => Error::EntitlementsUnavailable,
        Some("key_limit") => match (
            body.pointer("/error/details/limit").and_then(Value::as_u64),
            body.pointer("/error/details/active")
                .and_then(Value::as_u64),
        ) {
            (Some(limit), Some(active)) => Error::KeyLimit { limit, active },
            _ => Error::Refused,
        },
        _ => Error::Refused,
    }
}

#[cfg(test)]
mod tests;

fn paid_route(route: &str) -> bool {
    !matches!(
        route,
        "friendsList" | "friendsOwnKeys" | "friendsReceiverStatus"
    )
}

struct PaidCallGuard<'a> {
    calls: &'a Calls,
    expected: &'a codeterm_account::Session,
    paid: bool,
}

#[async_trait]
impl codeterm_account::account_mesh::callable::CallableGuard for PaidCallGuard<'_> {
    async fn current(&self) -> bool {
        if self.calls.service.require_neighbors_enabled().is_err()
            || self.paid && self.calls.require_paid().is_err()
        {
            return false;
        }
        crate::service::account::callable_admission::SessionGuard {
            service: &self.calls.service,
            expected: self.expected,
            neighbors: false,
        }
        .current()
        .await
    }
    async fn retired(&self) {
        let mut changes = self.calls.service.callable_admission_changes().subscribe();
        loop {
            if !self.current().await || changes.changed().await.is_err() {
                return;
            }
        }
    }
}
