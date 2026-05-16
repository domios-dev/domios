//! `BitwardenStore` — struct, constructors, session/auth helpers, JSON helpers.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::secrets::keychain::Keychain;
use crate::secrets::{SecretError, SecretStore, SecretString};

use super::exec::{BwExec, BwRunCtx, NoopBwExec};
use super::subprocess::SubprocessBwExec;
use super::{
    BW_KEYRING_LOGIN_EMAIL, BW_KEYRING_MASTER_PW, BW_KEYRING_SERVICE, BW_KEYRING_SESSION,
    DEFAULT_BW_SERVER,
};

pub struct BitwardenStore {
    pub(super) exec: Arc<dyn BwExec>,
    pub(super) keychain: Keychain,
    pub(super) session: RwLock<Option<SecretString>>,
    pub(super) unavailable_reason: Option<String>,
    pub(super) user: Mutex<Option<String>>,
    pub(super) server_url: Mutex<String>,
    /// When true, master password is saved on successful unlock so the
    /// daemon can auto-unlock after restart. Off by default — opt-in via
    /// `integrations.secrets.providers.bitwarden.persist_master_password`.
    pub persist_master_password: bool,
}

impl BitwardenStore {
    /// Production constructor: detect `bw`, build the OS keychain, restore
    /// any persisted session token. Falls back to an "Unavailable" store
    /// when `bw` is missing — every call returns `SecretError::Backend`.
    pub fn detect_and_build() -> Self {
        Self::detect_and_build_with_server(DEFAULT_BW_SERVER.to_string())
    }

    pub fn detect_and_build_with_server(server_url: String) -> Self {
        match SubprocessBwExec::detect() {
            Some(exec) => {
                let keychain = Keychain::new(BW_KEYRING_SERVICE);
                Self::new_with_server(Arc::new(exec), keychain, server_url)
            }
            None => Self::new_unavailable(
                "bw not found on PATH — install Bitwarden CLI \
                 (brew install bitwarden-cli, npm i -g @bitwarden/cli, scoop install bitwarden-cli)",
            ),
        }
    }

    pub fn new(exec: Arc<dyn BwExec>, keychain: Keychain) -> Self {
        Self::new_with_server(exec, keychain, DEFAULT_BW_SERVER.to_string())
    }

    pub fn new_with_server(
        exec: Arc<dyn BwExec>,
        keychain: Keychain,
        server_url: String,
    ) -> Self {
        let session = keychain
            .get(BW_KEYRING_SESSION)
            .ok()
            .map(SecretString::from);
        Self {
            exec,
            keychain,
            session: RwLock::new(session),
            unavailable_reason: None,
            user: Mutex::new(None),
            server_url: Mutex::new(server_url),
            persist_master_password: false,
        }
    }

    pub fn new_unavailable(reason: impl Into<String>) -> Self {
        Self {
            exec: Arc::new(NoopBwExec),
            keychain: Keychain::in_memory("bw not available"),
            session: RwLock::new(None),
            unavailable_reason: Some(reason.into()),
            user: Mutex::new(None),
            server_url: Mutex::new(DEFAULT_BW_SERVER.to_string()),
            persist_master_password: false,
        }
    }

    /// Returns the configured server URL (defaults to public vault).
    pub fn server_url(&self) -> String {
        self.server_url.lock().clone()
    }

    /// Update the Bitwarden server URL. If the new URL differs from the
    /// previous one and there is an active session, the caller is
    /// responsible for forcing a re-login (logout → login). The URL is
    /// applied to the bw CLI on the next unlock via `bw config server`.
    pub fn set_server_url(&self, new_url: String) -> bool {
        let mut g = self.server_url.lock();
        if *g == new_url {
            false
        } else {
            *g = new_url;
            true
        }
    }

    /// Try to restore an unlocked session from persisted credentials.
    /// Called once at construction. Silent — failures here just leave
    /// the store in its current state (typically Locked or LoggedOut),
    /// and the user can `unlock` manually to re-establish.
    pub async fn auto_unlock_from_keychain(&self) {
        if self.unavailable_reason.is_some() {
            return;
        }
        // If we already have a session AND bw_status agrees we're
        // unlocked, nothing to do. We treat any other state as "try
        // re-unlock" — sessions can expire silently.
        if let Ok(s) = self.bw_status().await {
            if s.status == "unlocked" {
                return;
            }
        }
        let master = match self.keychain.get(BW_KEYRING_MASTER_PW) {
            Ok(v) if !v.is_empty() => v,
            _ => return,
        };
        let email = self.keychain.get(BW_KEYRING_LOGIN_EMAIL).ok();
        let creds = crate::secrets::UnlockCredentials {
            master_password: SecretString::from(master),
            two_factor_token: None,
            email,
            api_key_client_id: None,
            api_key_client_secret: None,
        };
        match self.unlock(creds).await {
            Ok(()) => log::info!("[secrets/bw] auto-unlocked from stored master password"),
            Err(e) => log::warn!("[secrets/bw] auto-unlock failed: {}", e),
        }
    }

    pub(super) fn require_available(&self) -> Result<(), SecretError> {
        if let Some(reason) = &self.unavailable_reason {
            return Err(SecretError::Backend {
                message: reason.clone(),
            });
        }
        Ok(())
    }

    pub(super) async fn current_session(&self) -> Result<String, SecretError> {
        self.require_available()?;
        match self.session.read().await.as_ref() {
            Some(s) => Ok(s.expose_secret().to_string()),
            None => Err(SecretError::Locked),
        }
    }

    pub(super) async fn run_with_session(
        &self,
        args: Vec<&str>,
        stdin: Option<String>,
    ) -> Result<serde_json::Value, SecretError> {
        let session = match self.current_session().await {
            Ok(s) => s,
            Err(SecretError::Locked) => {
                self.try_unlock_from_file().await;
                self.current_session().await?
            }
            Err(e) => return Err(e),
        };
        let resp = self
            .exec
            .run(BwRunCtx {
                args,
                stdin,
                env: HashMap::new(),
                session: Some(&session),
            })
            .await
            .map_err(|e| SecretError::Backend { message: e })?;
        resp.into_data()
    }

    /// JIT auto-unlock: read ~/.codeterm/secrets/bitwarden-master.env on every
    /// locked op so the agent never has to see/think about unlock.
    pub(super) async fn try_unlock_from_file(&self) {
        if self.unavailable_reason.is_some() { return; }
        let path = match super::master_password_file_path() {
            Some(p) => p,
            None => return,
        };
        let master = match super::read_master_password_file(&path) {
            Some(s) if !s.is_empty() => s,
            _ => return,
        };
        let email = self.keychain.get(BW_KEYRING_LOGIN_EMAIL).ok();
        let creds = crate::secrets::UnlockCredentials {
            master_password: SecretString::from(master),
            two_factor_token: None,
            email,
            api_key_client_id: None,
            api_key_client_secret: None,
        };
        if let Err(e) = self.unlock(creds).await {
            log::warn!("[secrets/bw] JIT unlock from {} failed: {}", path.display(), e);
        }
    }

    pub(super) async fn run_no_session(
        &self,
        args: Vec<&str>,
        stdin: Option<String>,
        env: HashMap<String, String>,
    ) -> Result<serde_json::Value, SecretError> {
        self.require_available()?;
        let resp = self
            .exec
            .run(BwRunCtx {
                args,
                stdin,
                env,
                session: None,
            })
            .await
            .map_err(|e| SecretError::Backend { message: e })?;
        resp.into_data()
    }

    /// `bw status` — JSON {status, userEmail, …}. Locked / unauthenticated /
    /// unlocked. Used for status() and to decide which login path to take.
    pub(super) async fn bw_status(&self) -> Result<BwStatusJson, SecretError> {
        // bw status works without a session.
        let resp = self
            .exec
            .run(BwRunCtx {
                args: vec!["status"],
                stdin: None,
                env: HashMap::new(),
                session: self
                    .session
                    .read()
                    .await
                    .as_ref()
                    .map(|s| s.expose_secret().to_string())
                    .as_deref()
                    .map(|s| s),
            })
            .await
            .map_err(|e| SecretError::Backend { message: e })?;
        let data = resp.into_data()?;
        // bw status --response wraps the real payload in
        // { "object": "template", "template": { status, serverUrl, … } }
        // (modern) or returns it flat (legacy). The shared envelope
        // helper handles both shapes plus the {data: ...} list wrapper.
        serde_json::from_value(unwrap_data_envelope(data)).map_err(|e| SecretError::Backend {
            message: format!("parse bw status: {}", e),
        })
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct BwStatusJson {
    #[serde(default)]
    pub status: String,
    #[serde(default, rename = "userEmail")]
    pub user_email: Option<String>,
    #[serde(default, rename = "serverUrl")]
    pub server_url: Option<String>,
}

/// Unwrap the `{object: "list", data: [...]}` envelope that modern bw
/// versions wrap their `list` / `get` responses in. Falls back to the raw
/// value if no envelope is present (older bw or single-item endpoints).
pub(super) fn unwrap_data_envelope(value: serde_json::Value) -> serde_json::Value {
    if let Some(inner) = value.get("data") {
        // {object:"list", data: [...]}  /  {object:"item", data: {...}}
        return inner.clone();
    }
    if let Some(inner) = value.get("template") {
        return inner.clone();
    }
    value
}

/// Validate a Bitwarden server URL. Only http(s) schemes allowed; this
/// guards against accidental injection of arbitrary scheme handlers
/// before the URL reaches `bw config server`.
pub fn is_valid_http_url(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return false;
    }
    // Reject obviously malformed (no host part).
    let after_scheme = trimmed
        .splitn(2, "://")
        .nth(1)
        .unwrap_or("")
        .trim_start_matches('/');
    let host = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    !host.is_empty()
}
