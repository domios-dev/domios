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
    pub own_keys: tokio::sync::Mutex<Option<Value>>,
    pub receiver_status: tokio::sync::Mutex<Option<Value>>,
}

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
        let account = self.service.account().await.map_err(|_| Error::Transport)?;
        if super::super::same_session(account.session().await.as_ref(), expected) {
            Ok(())
        } else {
            Err(Error::Revoked)
        }
    }
}

#[async_trait]
impl AuthenticatedCall for Calls {
    fn firestore_failure(
        &self,
        operation: &'static str,
        target: &str,
        code: Option<i32>,
        kind: &str,
    ) {
        log::debug!(
            "[neighbor-listen] operation={operation} target={target} code={code:?} kind={kind}"
        );
    }
    async fn id_token(&self) -> Result<Zeroizing<String>> {
        self.session()
            .await
            .map(|session| Zeroizing::new(session.id_token.expose().to_owned()))
    }
    async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
        let cached_lookup = match route {
            "friendsOwnKeys" => Some(self.own_keys.lock().await),
            "friendsReceiverStatus" => Some(self.receiver_status.lock().await),
            _ => None,
        };
        if let Some(cached) = &cached_lookup {
            if let Some(value) = cached.as_ref() {
                self.local_session().await?;
                return Ok(value.clone());
            }
        }
        if matches!(
            route,
            "friendsEnrollVerifierKey" | "friendsRevokeVerifierKey" | "friendsDisableReceiving"
        ) {
            self.own_keys.lock().await.take();
            self.receiver_status.lock().await.take();
        }
        let session = self.session().await?;
        let transport =
            HttpCallable::new(codeterm_account::firebase_endpoints::current().functions)
                .map_err(|_| Error::Transport)?;
        let response = transport.call(route, session.id_token.expose(), data).await;
        let body = match response {
            Ok(body) => body,
            Err(CallableFailure::Status {
                body: Some(body), ..
            }) => {
                if codeterm_account::session_revocation::is_session_revoked(&body) {
                    self.service.session_revocations().revoked(&session).await;
                    return Err(Error::SessionRevoked);
                }
                return Err(refusal(&body));
            }
            Err(_) => return Err(Error::Transport),
        };
        self.validate_session(&session).await?;
        let result = body.get("result").cloned().ok_or(Error::Transport)?;
        if let Some(mut cached) = cached_lookup {
            *cached = Some(result.clone());
        }
        Ok(result)
    }
}

fn refusal(body: &Value) -> Error {
    match body
        .pointer("/error/details/reason")
        .and_then(Value::as_str)
        .or_else(|| body.pointer("/error/message").and_then(Value::as_str))
    {
        Some("neighbor_daily_limit") => {
            match (
                body.pointer("/error/details/limit").and_then(Value::as_u64),
                body.pointer("/error/details/resets_at")
                    .and_then(Value::as_u64),
            ) {
                (Some(limit), Some(resets_at)) => Error::DailyLimit { limit, resets_at },
                _ => Error::Refused,
            }
        }
        Some("limit_out_of_range") => Error::LimitOutOfRange,
        Some("invalid_request") => Error::InvalidRequest,
        Some("friendship_not_found") => Error::FriendshipNotFound,
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
        Some("friends_locked") => Error::Locked,
        Some("key_revoked" | "unknown_key") => Error::Revoked,
        Some("key_not_found") => Error::KeyNotFound,
        _ => Error::Refused,
    }
}

#[cfg(test)]
mod tests;
