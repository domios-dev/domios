use super::AccountCredentials;
use crate::service::ServiceContext;
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use codeterm_account::account_mesh::{wire::SessionRevokedResult, AccountMeshRefusal};
use codeterm_account::{Identity, Secret, Session};
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const INTENTS_KEY: &str = "domios-account-pending-server-stop";
const STOP_DEADLINE: Duration = Duration::from_secs(15);
pub(crate) type StopResult = Result<Option<SessionRevokedResult>, AccountMeshRefusal>;

#[derive(Default)]
pub(super) struct LogoutCell(pub tokio::sync::Mutex<()>);

#[derive(Clone, Serialize, Deserialize)]
struct Intent {
    identity: Identity,
    login_generation: u64,
    expires_at: Option<u64>,
    auth_time: Option<u64>,
    id_token: Option<String>,
}

impl Intent {
    fn captured(session: &Session, now: u64) -> Self {
        Self {
            identity: session.identity.clone(),
            login_generation: session.login_generation,
            expires_at: session.expires_at,
            auth_time: auth_time(session),
            id_token: session
                .expires_at
                .filter(|expiry| *expiry > now)
                .map(|_| session.id_token.expose().to_owned()),
        }
    }

    fn expire(&mut self, now: u64) {
        if self.expires_at.is_none_or(|expiry| expiry <= now) {
            self.id_token = None;
        }
    }

    fn session(&self) -> Option<Session> {
        Some(Session {
            identity: self.identity.clone(),
            login_generation: self.login_generation,
            expires_at: self.expires_at,
            id_token: Secret::new(self.id_token.as_ref()?.clone()),
        })
    }
}

#[async_trait]
trait StopPorts: Send + Sync {
    async fn load(&self) -> Result<Vec<Intent>, AccountMeshRefusal>;
    async fn save(&self, intents: &[Intent]) -> Result<(), AccountMeshRefusal>;
    async fn revoke(&self, session: &Session) -> Result<SessionRevokedResult, AccountMeshRefusal>;
    async fn revoke_before(&self, session: &Session, cutoff: u64)
        -> Result<(), AccountMeshRefusal>;
}

struct Ports {
    credentials: Arc<AccountCredentials>,
    mesh: Option<Arc<super::super::account_mesh::AccountMeshService>>,
}

#[async_trait]
impl StopPorts for Ports {
    async fn load(&self) -> Result<Vec<Intent>, AccountMeshRefusal> {
        let secret = self
            .credentials
            .load_named(INTENTS_KEY.into())
            .await
            .map_err(|_| AccountMeshRefusal::Unavailable)?;
        secret
            .map(|secret| {
                serde_json::from_str(secret.expose()).map_err(|_| AccountMeshRefusal::Unavailable)
            })
            .unwrap_or(Ok(Vec::new()))
    }

    async fn save(&self, intents: &[Intent]) -> Result<(), AccountMeshRefusal> {
        if intents.is_empty() {
            return self
                .credentials
                .delete_named(INTENTS_KEY.into())
                .await
                .map_err(|_| AccountMeshRefusal::Unavailable);
        }
        let secret = Secret::new(
            serde_json::to_string(intents).map_err(|_| AccountMeshRefusal::Unavailable)?,
        );
        self.credentials
            .save_persistent(INTENTS_KEY.into(), &secret)
            .await
            .map_err(|_| AccountMeshRefusal::Unavailable)
    }

    async fn revoke(&self, session: &Session) -> Result<SessionRevokedResult, AccountMeshRefusal> {
        self.mesh
            .as_ref()
            .ok_or(AccountMeshRefusal::Unavailable)?
            .mesh
            .revoke_outgoing_session(session)
            .await
    }

    async fn revoke_before(
        &self,
        session: &Session,
        cutoff: u64,
    ) -> Result<(), AccountMeshRefusal> {
        self.mesh
            .as_ref()
            .ok_or(AccountMeshRefusal::Unavailable)?
            .mesh
            .revoke_prior_sessions(session, cutoff)
            .await
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn auth_time(session: &Session) -> Option<u64> {
    let payload = session.id_token.expose().split('.').nth(1)?;
    let value: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    value.get("auth_time")?.as_u64().filter(|time| *time > 0)
}

async fn bounded<T>(
    call: impl std::future::Future<Output = Result<T, AccountMeshRefusal>>,
) -> Result<T, AccountMeshRefusal> {
    tokio::time::timeout(STOP_DEADLINE, call)
        .await
        .unwrap_or(Err(AccountMeshRefusal::Offline))
}

async fn capture_and_revoke(ports: &dyn StopPorts, session: &Session, now: u64) -> StopResult {
    let mut intents = match ports.load().await {
        Ok(intents) => intents,
        Err(error) => {
            log::debug!("Account server stop intent unavailable: error_kind={error:?}");
            return bounded(ports.revoke(session)).await.map(Some);
        }
    };
    for intent in &mut intents {
        intent.expire(now);
    }
    let index = match intents.iter().position(|intent| {
        intent.identity.uid == session.identity.uid
            && intent.login_generation == session.login_generation
            && intent.expires_at == session.expires_at
    }) {
        Some(index) => index,
        None => {
            intents.push(Intent::captured(session, now));
            intents.len() - 1
        }
    };
    let saved = ports.save(&intents).await;
    let result = bounded(ports.revoke(session)).await;
    if result.is_ok() {
        intents.remove(index);
        ports.save(&intents).await?;
    } else {
        saved?;
    }
    result.map(Some)
}

async fn retry(
    ports: &dyn StopPorts,
    now: u64,
    fresh: Option<&Session>,
) -> Result<(), AccountMeshRefusal> {
    let mut intents = ports.load().await?;
    if intents.is_empty() {
        return Ok(());
    }
    for intent in &mut intents {
        intent.expire(now);
    }
    ports.save(&intents).await?;
    while let Some(intent) = intents.first() {
        if let Some(fresh) = fresh.filter(|session| session.identity.uid == intent.identity.uid) {
            let cutoff = auth_time(fresh).ok_or(AccountMeshRefusal::Invalid)?;
            if intent.auth_time.is_none_or(|previous| previous >= cutoff) {
                return Err(AccountMeshRefusal::ReauthRequired);
            }
            bounded(ports.revoke_before(fresh, cutoff)).await?;
        } else if let Some(session) = intent.session() {
            bounded(ports.revoke(&session)).await?;
        } else {
            return Err(AccountMeshRefusal::Unauthenticated);
        }
        intents.remove(0);
        ports.save(&intents).await?;
    }
    Ok(())
}

pub(super) struct SignInAdmission(pub std::sync::Weak<ServiceContext>);

#[async_trait]
impl codeterm_account::SessionAdmission for SignInAdmission {
    async fn admit(&self, session: &Session) -> codeterm_account::Result<()> {
        self.0
            .upgrade()
            .ok_or(codeterm_account::Error::Cancelled)?
            .retry_account_sign_out(Some(session))
            .await
            .map_err(|_| codeterm_account::Error::Transport)
    }
}

impl ServiceContext {
    pub(super) async fn queue_server_stop(
        &self,
        session: &Session,
    ) -> Result<(), AccountMeshRefusal> {
        let credentials = self
            .account_credentials()
            .await
            .map_err(|_| AccountMeshRefusal::Unavailable)?;
        let ports = Ports {
            credentials,
            mesh: None,
        };
        let mut intents = ports.load().await?;
        let now = now();
        for intent in &mut intents {
            intent.expire(now);
        }
        intents.push(Intent::captured(session, now));
        ports.save(&intents).await
    }

    pub(super) async fn revoke_signed_out_session(
        &self,
        mesh: Option<Arc<super::super::account_mesh::AccountMeshService>>,
        session: Option<&Session>,
    ) -> StopResult {
        let credentials = self
            .account_credentials()
            .await
            .map_err(|_| AccountMeshRefusal::Unavailable)?;
        let ports = Ports { credentials, mesh };
        match session {
            Some(session) => capture_and_revoke(&ports, session, now()).await,
            None => retry(&ports, now(), None).await.map(|_| None),
        }
    }

    pub(crate) async fn retry_account_sign_out(
        &self,
        fresh: Option<&Session>,
    ) -> Result<(), AccountMeshRefusal> {
        let cell = self.extensions.get_or_init::<LogoutCell>(Default::default);
        let _guard = cell.0.lock().await;
        let credentials = self
            .account_credentials()
            .await
            .map_err(|_| AccountMeshRefusal::Unavailable)?;
        let mesh = self.account_mesh().await.ok();
        retry(&Ports { credentials, mesh }, now(), fresh).await
    }
}

#[cfg(test)]
mod tests;
