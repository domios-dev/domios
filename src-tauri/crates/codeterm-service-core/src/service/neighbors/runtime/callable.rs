use crate::service::ServiceContext;
use async_trait::async_trait;
use codeterm_account::account_mesh::callable::{CallableFailure, CallableTransport, HttpCallable};
use codeterm_neighbors::{adapter::AuthenticatedCall, Error, Result};
use serde_json::Value;
use zeroize::Zeroizing;

pub(super) struct Calls {
    pub service: ServiceContext,
    pub uid: String,
    pub generation: u64,
}

impl Calls {
    async fn session(&self) -> Result<codeterm_account::Session> {
        let account = self.service.account().await.map_err(|_| Error::Transport)?;
        let session = account
            .fresh_session()
            .await
            .map_err(|_| Error::Transport)?
            .ok_or(Error::Revoked)?;
        if session.identity.uid != self.uid || session.login_generation != self.generation {
            return Err(Error::Revoked);
        }
        Ok(session)
    }
}

#[async_trait]
impl AuthenticatedCall for Calls {
    async fn id_token(&self) -> Result<Zeroizing<String>> {
        self.session()
            .await
            .map(|session| Zeroizing::new(session.id_token.expose().to_owned()))
    }
    async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
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
                    return Err(Error::Revoked);
                }
                return Err(
                    match body
                        .pointer("/error/details/reason")
                        .and_then(Value::as_str)
                    {
                        Some("friends_locked") => Error::Locked,
                        Some("key_revoked" | "unknown_key") => Error::Revoked,
                        Some("key_not_found") => Error::KeyNotFound,
                        _ => Error::Refused,
                    },
                );
            }
            Err(_) => return Err(Error::Transport),
        };
        self.session().await?;
        body.get("result").cloned().ok_or(Error::Transport)
    }
}
