//! Bitwarden adapter for `SecretStore`.
//!
//! Shells out to the official `bw` CLI — we never re-implement vault crypto.
//!
//! Architecture: every `bw` invocation goes through the `BwExec` trait so
//! tests inject canned responses without spawning a real subprocess. The
//! production impl, `SubprocessBwExec`, runs `bw` via `tokio::process` with
//! `--nointeraction --response` flags so output is always JSON
//! (`{"success":bool,"data":…,"message":"…"}`) and stdin/argv are
//! controlled. Master password and session token always travel via env or
//! stdin — never argv (visible to other users via `ps`).
//!
//! Persistence: the unlocked session token lands in the OS keychain via
//! `Keychain` so a daemon restart doesn't force the user to re-unlock. If
//! the keychain isn't available (headless Linux, feature off, probe fail),
//! we keep the token in process only and the UI marks the session
//! `transient: true`.
//!
//! Future work the plan calls out and that we deliberately defer past
//! Phase 2:
//! - `bw serve` hot path. Phase 2 spawns one `bw` per call (~500 ms each);
//!   `bw serve` can reduce that to localhost-REST round trips. The
//!   `BwExec` indirection means we can swap in a serve-backed exec later
//!   without touching `BitwardenStore`.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::process::Command;
use tokio::sync::RwLock;

use super::factory::SecretBackendFactory;
use super::{
    validate_name, CollectionRef, ListFilter, OrganizationRef, SaveRequest, SecretError,
    SecretRef, SecretStore, SecretString, SecretValue, StoreStatus, UnlockCredentials,
};
use super::keychain::Keychain;
#[cfg(test)]
use super::keychain::KeychainError;

pub struct BitwardenFactory;

impl SecretBackendFactory for BitwardenFactory {
    fn id(&self) -> &'static str {
        "bitwarden"
    }
    fn label(&self) -> &'static str {
        "Bitwarden"
    }
    fn description(&self) -> &'static str {
        "Vault accessed via the bw CLI. Supports vault.bitwarden.com and self-hosted Vaultwarden."
    }
    fn build(&self, cfg: &serde_json::Value) -> Box<dyn SecretStore> {
        let parsed: crate::config::schema::BitwardenSettings =
            serde_json::from_value(cfg.clone()).unwrap_or_default();
        let server = if parsed.server_url.trim().is_empty() {
            DEFAULT_BW_SERVER.to_string()
        } else {
            parsed.server_url
        };
        let mut store = BitwardenStore::detect_and_build_with_server(server);
        store.persist_master_password = parsed.persist_master_password;
        Box::new(store)
    }
}

const BW_KEYRING_SERVICE: &str = "codeterm.secrets.bitwarden";
const BW_KEYRING_SESSION: &str = "session";
const BW_KEYRING_API_CLIENT_ID: &str = "apikey:client_id";
const BW_KEYRING_API_CLIENT_SECRET: &str = "apikey:client_secret";
/// Persisted master password for auto-unlock after daemon restart or
/// session expiry. Lives in the same keychain as the session token (file
/// in dev, OS keychain in release).
const BW_KEYRING_MASTER_PW: &str = "master_password";
const BW_KEYRING_LOGIN_EMAIL: &str = "login_email";

/// Default Bitwarden server URL — public hosted vault.
pub const DEFAULT_BW_SERVER: &str = "https://vault.bitwarden.com";

/// One execution of `bw <args>`. Tests stub this to feed canned JSON
/// without spawning a process.
#[async_trait]
pub trait BwExec: Send + Sync {
    /// Run `bw <args>` with optional stdin + extra env. Returns the parsed
    /// response wrapper. Implementations always pass `--nointeraction
    /// --response` at the end of `args`.
    async fn run(&self, ctx: BwRunCtx<'_>) -> Result<BwResponse, String>;
}

/// Inputs to a single `bw` invocation. Borrowed strings keep the call site
/// terse without cloning every arg.
pub struct BwRunCtx<'a> {
    pub args: Vec<&'a str>,
    pub stdin: Option<String>,
    pub env: HashMap<String, String>,
    pub session: Option<&'a str>,
}

/// Parsed `--response` JSON wrapper.
/// `bw` shapes:
///   success: { "success": true,  "data": <value>          }
///   failure: { "success": false, "data": null, "message": "...", "errorCode": ... }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BwResponse {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub data: serde_json::Value,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default, rename = "errorCode")]
    pub error_code: Option<i64>,
}

impl BwResponse {
    pub fn into_data(self) -> Result<serde_json::Value, SecretError> {
        if self.success {
            Ok(self.data)
        } else {
            Err(map_bw_message(self.message.as_deref().unwrap_or("")))
        }
    }
}

/// Translate a `bw` error message into a `SecretError`. Substring matching
/// is the documented vendor-stable signal — Bitwarden has used these
/// strings for years and they survive UI rewrites.
pub fn map_bw_message(msg: &str) -> SecretError {
    let lower = msg.to_lowercase();
    if lower.contains("vault is locked") {
        SecretError::Locked
    } else if lower.contains("you are not logged in") {
        SecretError::LoggedOut
    } else if lower.contains("not found") {
        SecretError::NotFound
    } else if lower.contains("more than one result") {
        SecretError::Ambiguous { candidates: vec![] }
    } else if lower.contains("already") && lower.contains("exists") {
        SecretError::AlreadyExists
    } else {
        SecretError::Backend {
            message: if msg.is_empty() { "bw failed".into() } else { msg.into() },
        }
    }
}

// ── Real subprocess exec ────────────────────────────────────────────

pub struct SubprocessBwExec {
    bw_path: std::path::PathBuf,
}

impl SubprocessBwExec {
    pub fn new(bw_path: std::path::PathBuf) -> Self {
        Self { bw_path }
    }

    pub fn detect() -> Option<Self> {
        which::which("bw").ok().map(Self::new)
    }
}

#[async_trait]
impl BwExec for SubprocessBwExec {
    async fn run(&self, ctx: BwRunCtx<'_>) -> Result<BwResponse, String> {
        use tokio::io::AsyncWriteExt;

        let cmd_label = ctx.args.first().copied().unwrap_or("?").to_string();
        let mut cmd = Command::new(&self.bw_path);
        cmd.args(&ctx.args);
        cmd.arg("--nointeraction").arg("--response");
        cmd.envs(&ctx.env);
        if let Some(session) = ctx.session {
            cmd.env("BW_SESSION", session);
        }
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("spawn bw: {}", e))?;
        if let Some(stdin) = ctx.stdin {
            if let Some(mut sin) = child.stdin.take() {
                sin.write_all(stdin.as_bytes())
                    .await
                    .map_err(|e| format!("write stdin: {}", e))?;
            }
        }
        let output = child
            .wait_with_output()
            .await
            .map_err(|e| format!("wait bw: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // With --response, bw prints the wrapper JSON to stdout for both
        // success and failure. If parsing fails, fall through to a raw
        // Backend error so the caller sees something useful instead of
        // silently treating it as success.
        if let Ok(resp) = serde_json::from_str::<BwResponse>(stdout.trim()) {
            if !resp.success {
                log::warn!(
                    "[secrets/bw] {} failed: msg={:?} errorCode={:?} stderr={}",
                    cmd_label,
                    resp.message,
                    resp.error_code,
                    stderr.trim()
                );
            }
            return Ok(resp);
        }

        let combined = if !stderr.is_empty() {
            stderr.clone()
        } else if !stdout.is_empty() {
            stdout.clone()
        } else {
            format!("bw exited with {}", output.status)
        };
        log::warn!(
            "[secrets/bw] {} produced unparseable output: status={} stdout={} stderr={}",
            cmd_label,
            output.status,
            stdout.trim(),
            stderr.trim()
        );
        Ok(BwResponse {
            success: false,
            data: serde_json::Value::Null,
            message: Some(combined),
            error_code: None,
        })
    }
}

// ── BitwardenStore ──────────────────────────────────────────────────

pub struct BitwardenStore {
    exec: Arc<dyn BwExec>,
    keychain: Keychain,
    session: RwLock<Option<SecretString>>,
    unavailable_reason: Option<String>,
    user: Mutex<Option<String>>,
    server_url: Mutex<String>,
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
        let creds = UnlockCredentials {
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

    fn require_available(&self) -> Result<(), SecretError> {
        if let Some(reason) = &self.unavailable_reason {
            return Err(SecretError::Backend {
                message: reason.clone(),
            });
        }
        Ok(())
    }

    async fn current_session(&self) -> Result<String, SecretError> {
        self.require_available()?;
        match self.session.read().await.as_ref() {
            Some(s) => Ok(s.expose_secret().to_string()),
            None => Err(SecretError::Locked),
        }
    }

    async fn run_with_session(
        &self,
        args: Vec<&str>,
        stdin: Option<String>,
    ) -> Result<serde_json::Value, SecretError> {
        let session = self.current_session().await?;
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

    async fn run_no_session(
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
    async fn bw_status(&self) -> Result<BwStatusJson, SecretError> {
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
struct BwStatusJson {
    #[serde(default)]
    status: String,
    #[serde(default, rename = "userEmail")]
    user_email: Option<String>,
    #[serde(default, rename = "serverUrl")]
    server_url: Option<String>,
}

/// Unwrap the `{object: "list", data: [...]}` envelope that modern bw
/// versions wrap their `list` / `get` responses in. Falls back to the raw
/// value if no envelope is present (older bw or single-item endpoints).
fn unwrap_data_envelope(value: serde_json::Value) -> serde_json::Value {
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

// Sentinel exec used when bw is not on PATH — every call errors with the
// reason captured in BitwardenStore.unavailable_reason.
struct NoopBwExec;

#[async_trait]
impl BwExec for NoopBwExec {
    async fn run(&self, _ctx: BwRunCtx<'_>) -> Result<BwResponse, String> {
        Err("bw not available".into())
    }
}

// ── SecretStore impl ────────────────────────────────────────────────

#[async_trait]
impl SecretStore for BitwardenStore {
    fn id(&self) -> &'static str {
        "bitwarden"
    }

    async fn status(&self) -> StoreStatus {
        if let Some(reason) = &self.unavailable_reason {
            return StoreStatus::Unavailable {
                reason: reason.clone(),
            };
        }
        let endpoint = Some(self.server_url.lock().clone());
        match self.bw_status().await {
            Ok(s) => match s.status.as_str() {
                "unlocked" => {
                    if let Some(email) = s.user_email.clone() {
                        *self.user.lock() = Some(email);
                    }
                    StoreStatus::Unlocked {
                        user: self.user.lock().clone(),
                        transient: !self.keychain.is_available(),
                        endpoint: s.server_url.or(endpoint),
                    }
                }
                "locked" => StoreStatus::Locked { endpoint },
                _ => StoreStatus::LoggedOut { endpoint },
            },
            Err(SecretError::Backend { message }) => StoreStatus::Unavailable {
                reason: message,
            },
            Err(SecretError::Locked) => StoreStatus::Locked { endpoint },
            Err(SecretError::LoggedOut) => StoreStatus::LoggedOut { endpoint },
            Err(other) => StoreStatus::Unavailable {
                reason: other.to_string(),
            },
        }
    }

    async fn unlock(&self, creds: UnlockCredentials) -> Result<(), SecretError> {
        self.require_available()?;
        if creds.master_password.is_empty()
            && creds.api_key_client_id.is_none()
        {
            return Err(SecretError::BadRequest {
                message: "master password (or API-key creds) required".into(),
            });
        }

        // Validate server URL — guard against malformed config that would
        // otherwise be passed verbatim to `bw config server`.
        let server = self.server_url.lock().clone();
        if !is_valid_http_url(&server) {
            return Err(SecretError::BadRequest {
                message: format!("invalid bitwarden server URL: {}", server),
            });
        }

        let status = self.bw_status().await.ok();
        let is_logged_in = status
            .as_ref()
            .map(|s| s.status != "unauthenticated")
            .unwrap_or(false);

        // Point bw at the configured server BEFORE any login. Safe to
        // re-run on every unlock — bw treats it as idempotent. If the URL
        // differs from a logged-in server, force a logout first so login
        // talks to the right host.
        let current_server = status
            .as_ref()
            .and_then(|s| s.server_url.clone())
            .unwrap_or_default();
        if is_logged_in && !current_server.is_empty() && current_server != server {
            log::info!(
                "[secrets/bw] server URL changed ({} -> {}), forcing logout",
                current_server,
                server
            );
            let _ = self
                .run_no_session(vec!["logout"], None, HashMap::new())
                .await;
            *self.session.write().await = None;
            let _ = self.keychain.delete(BW_KEYRING_SESSION);
        }
        // Apply the server config only when it actually changes. bw
        // refuses to update the server while logged in to it ("Logout
        // required before server config update"), so re-running config
        // server with the same URL is both pointless AND breaks the
        // common no-op login flow.
        if current_server != server {
            if let Err(e) = self
                .run_no_session(vec!["config", "server", &server], None, HashMap::new())
                .await
            {
                log::warn!("[secrets/bw] config server '{}' failed: {}", server, e);
                return Err(SecretError::Backend {
                    message: format!("could not point bw at {}: {}", server, e),
                });
            }
        }

        // Re-read login state after potential logout above.
        let status = self.bw_status().await.ok();
        let is_logged_in = status
            .as_ref()
            .map(|s| s.status != "unauthenticated")
            .unwrap_or(false);

        if !is_logged_in {
            // Need to log in first. "You are already logged in" is bw's
            // way of saying "skip the login step" — treat as soft success
            // so we proceed to unlock with the existing session.
            let login_result: Result<serde_json::Value, SecretError> =
                if let (Some(client_id), Some(client_secret)) =
                    (&creds.api_key_client_id, &creds.api_key_client_secret)
                {
                    let mut env = HashMap::new();
                    env.insert("BW_CLIENTID".into(), client_id.expose_secret().into());
                    env.insert(
                        "BW_CLIENTSECRET".into(),
                        client_secret.expose_secret().into(),
                    );
                    let r = self.run_no_session(vec!["login", "--apikey"], None, env).await;
                    if r.is_ok() {
                        let _ = self
                            .keychain
                            .set(BW_KEYRING_API_CLIENT_ID, client_id.expose_secret());
                        let _ = self
                            .keychain
                            .set(BW_KEYRING_API_CLIENT_SECRET, client_secret.expose_secret());
                    }
                    r
                } else if let Some(email) = creds.email.as_deref() {
                    let mut env = HashMap::new();
                    env.insert(
                        "BW_PASSWORD".into(),
                        creds.master_password.expose_secret().to_string(),
                    );
                    let mut args: Vec<&str> =
                        vec!["login", email, "--passwordenv", "BW_PASSWORD"];
                    if let Some(tok) = creds.two_factor_token.as_ref() {
                        args.push("--method");
                        args.push("0");
                        args.push("--code");
                        args.push(tok.expose_secret());
                    }
                    self.run_no_session(args, None, env).await
                } else {
                    return Err(SecretError::BadRequest {
                        message: "either email + master_password or api_key_* required".into(),
                    });
                };

            if let Err(SecretError::Backend { message }) = &login_result {
                if message.to_lowercase().contains("already logged in") {
                    log::info!(
                        "[secrets/bw] login skipped — bw reports already logged in"
                    );
                } else {
                    return Err(SecretError::Backend { message: message.clone() });
                }
            } else if let Err(other) = login_result {
                return Err(other);
            }
        }

        // Now unlock to get the session token.
        let mut env = HashMap::new();
        env.insert(
            "BW_PASSWORD".into(),
            creds.master_password.expose_secret().to_string(),
        );
        let unlock_data = self
            .run_no_session(
                vec!["unlock", "--passwordenv", "BW_PASSWORD", "--raw"],
                None,
                env,
            )
            .await?;

        // bw unlock --raw output shape varies by version:
        //   modern: { "object": "unlock", "raw": "<token>", ... }
        //   modern wrapped: { "template": { "raw": "<token>" } }
        //   legacy: just the raw token string at the top level
        let token_owned: String = unlock_data
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| {
                unlock_data
                    .get("raw")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .or_else(|| {
                unlock_data
                    .get("template")
                    .and_then(|t| t.get("raw"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .ok_or_else(|| SecretError::Backend {
                message: format!(
                    "bw unlock returned unexpected payload shape: {}",
                    unlock_data
                ),
            })?;
        let token = token_owned.trim().to_string();
        if token.is_empty() {
            return Err(SecretError::Backend {
                message: "bw unlock returned empty session".into(),
            });
        }

        *self.session.write().await = Some(SecretString::from(token.clone()));
        let _ = self.keychain.set(BW_KEYRING_SESSION, &token);
        // Master password persistence is gated by config so the user
        // explicitly opts into the trade-off (convenient auto-unlock vs.
        // plaintext-on-disk in dev / key-encrypted-at-rest in release).
        // API-key flow already persists its own client creds, so the
        // password is never needed for that path.
        if self.persist_master_password && creds.api_key_client_id.is_none() {
            let _ = self
                .keychain
                .set(BW_KEYRING_MASTER_PW, creds.master_password.expose_secret());
            if let Some(email) = creds.email.as_deref() {
                let _ = self.keychain.set(BW_KEYRING_LOGIN_EMAIL, email);
            }
        }
        Ok(())
    }

    async fn lock(&self) -> Result<(), SecretError> {
        self.require_available()?;
        let _ = self.run_no_session(vec!["lock"], None, HashMap::new()).await;
        *self.session.write().await = None;
        let _ = self.keychain.delete(BW_KEYRING_SESSION);
        // Master password stays — unlock without prompting again.
        Ok(())
    }

    async fn logout(&self) -> Result<(), SecretError> {
        self.require_available()?;
        let _ = self
            .run_no_session(vec!["logout"], None, HashMap::new())
            .await;
        *self.session.write().await = None;
        *self.user.lock() = None;
        let _ = self.keychain.delete(BW_KEYRING_SESSION);
        let _ = self.keychain.delete(BW_KEYRING_API_CLIENT_ID);
        let _ = self.keychain.delete(BW_KEYRING_API_CLIENT_SECRET);
        let _ = self.keychain.delete(BW_KEYRING_MASTER_PW);
        let _ = self.keychain.delete(BW_KEYRING_LOGIN_EMAIL);
        Ok(())
    }

    async fn save(&self, req: SaveRequest) -> Result<SecretRef, SecretError> {
        validate_name(&req.name)?;
        if req.value.is_empty() {
            return Err(SecretError::BadRequest {
                message: "value must not be empty".into(),
            });
        }

        // If overwriting, look up the existing item id first; bw create will
        // happily insert a duplicate-name item, which we never want.
        let existing = self.find_by_name(&req.name).await.ok();
        if existing.is_some() && !req.overwrite {
            return Err(SecretError::AlreadyExists);
        }

        let folder_id = match &req.folder {
            Some(f) => Some(self.resolve_or_create_folder(f).await?),
            None => None,
        };
        let cipher = build_login_cipher(
            &req,
            existing.as_ref().map(|c| c.id.as_str()),
            folder_id.as_deref(),
        );
        let json_body = serde_json::to_string(&cipher).map_err(|e| SecretError::Backend {
            message: format!("encode cipher: {}", e),
        })?;
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            json_body.as_bytes(),
        );

        let data = if let Some(c) = &existing {
            self.run_with_session(vec!["edit", "item", &c.id, &encoded], None)
                .await?
        } else {
            self.run_with_session(vec!["create", "item", &encoded], None)
                .await?
        };

        // Sync so subsequent list/get see the change without a manual sync.
        let _ = self.run_with_session(vec!["sync"], None).await;

        let saved: BwCipher = serde_json::from_value(unwrap_data_envelope(data))
            .map_err(|e| SecretError::Backend {
                message: format!("parse saved item: {}", e),
            })?;
        Ok(saved.into_ref())
    }

    async fn get(&self, name_or_id: &str) -> Result<SecretValue, SecretError> {
        validate_name(name_or_id).ok();
        let item = self.fetch_item(name_or_id).await?;
        let value = item
            .login
            .as_ref()
            .and_then(|l| l.password.clone())
            .ok_or_else(|| SecretError::Backend {
                message: "item has no login.password field".into(),
            })?;
        Ok(SecretValue {
            item_id: item.id.clone(),
            name: item.name.clone(),
            value: SecretString::from(value),
            notes: item.notes.clone(),
        })
    }

    async fn list(&self, filter: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
        let mut args: Vec<String> = vec!["list".into(), "items".into()];
        if let Some(org) = &filter.organization {
            args.push("--organizationid".into());
            args.push(org.clone());
        }
        if let Some(coll) = &filter.collection {
            args.push("--collectionid".into());
            args.push(coll.clone());
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let data = self.run_with_session(arg_refs, None).await?;
        let mut items: Vec<BwCipher> =
            serde_json::from_value(unwrap_data_envelope(data)).map_err(|e| SecretError::Backend {
                message: format!("parse list: {}", e),
            })?;
        if let Some(folder) = &filter.folder {
            // Bitwarden references folders by id; we accept the human name.
            let folder_id = self.resolve_folder_id(folder).await?;
            items.retain(|c| c.folder_id.as_deref() == folder_id.as_deref());
        }
        items.retain(|c| c.cipher_type == 1); // logins only
        items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        if let Some(limit) = filter.limit {
            items.truncate(limit);
        }
        Ok(items.into_iter().map(|c| c.into_ref()).collect())
    }

    async fn search(&self, query: &str) -> Result<Vec<SecretRef>, SecretError> {
        let q = query.trim();
        if q.is_empty() {
            return Err(SecretError::BadRequest {
                message: "query must not be empty".into(),
            });
        }
        let data = self
            .run_with_session(vec!["list", "items", "--search", q], None)
            .await?;
        let items: Vec<BwCipher> =
            serde_json::from_value(unwrap_data_envelope(data)).map_err(|e| SecretError::Backend {
                message: format!("parse search: {}", e),
            })?;
        Ok(items
            .into_iter()
            .filter(|c| c.cipher_type == 1)
            .map(|c| c.into_ref())
            .collect())
    }

    async fn delete(&self, name_or_id: &str) -> Result<(), SecretError> {
        let item = self.fetch_item(name_or_id).await?;
        self.run_with_session(
            vec!["delete", "item", &item.id, "--permanent"],
            None,
        )
        .await?;
        let _ = self.run_with_session(vec!["sync"], None).await;
        Ok(())
    }

    async fn organizations(&self) -> Result<Vec<OrganizationRef>, SecretError> {
        let data = self
            .run_with_session(vec!["list", "organizations"], None)
            .await?;
        let raw: Vec<serde_json::Value> =
            serde_json::from_value(unwrap_data_envelope(data)).map_err(|e| SecretError::Backend {
                message: format!("parse orgs: {}", e),
            })?;
        Ok(raw
            .into_iter()
            .filter_map(|v| {
                Some(OrganizationRef {
                    id: v.get("id")?.as_str()?.to_string(),
                    name: v.get("name")?.as_str()?.to_string(),
                })
            })
            .collect())
    }

    async fn sync(&self) -> Result<(), SecretError> {
        // bw sync requires an unlocked session. We surface failures so
        // callers can tell the difference between "synced ok" and "vault
        // is locked" (which would silently leave stale data).
        self.run_with_session(vec!["sync"], None).await?;
        Ok(())
    }

    async fn init(&self) -> Result<(), SecretError> {
        self.auto_unlock_from_keychain().await;
        Ok(())
    }

    async fn collections(
        &self,
        organization_id: Option<&str>,
    ) -> Result<Vec<CollectionRef>, SecretError> {
        let mut args: Vec<&str> = vec!["list", "collections"];
        if let Some(org) = organization_id {
            args.push("--organizationid");
            args.push(org);
        }
        let data = self.run_with_session(args, None).await?;
        let raw: Vec<serde_json::Value> =
            serde_json::from_value(unwrap_data_envelope(data)).map_err(|e| SecretError::Backend {
                message: format!("parse collections: {}", e),
            })?;
        Ok(raw
            .into_iter()
            .filter_map(|v| {
                Some(CollectionRef {
                    id: v.get("id")?.as_str()?.to_string(),
                    name: v.get("name")?.as_str()?.to_string(),
                    organization_id: v
                        .get("organizationId")
                        .and_then(|x| x.as_str())
                        .map(String::from),
                })
            })
            .collect())
    }
}

// ── Folder / item helpers ──────────────────────────────────────────

impl BitwardenStore {
    async fn fetch_item(&self, name_or_id: &str) -> Result<BwCipher, SecretError> {
        let data = self
            .run_with_session(vec!["get", "item", name_or_id], None)
            .await?;
        serde_json::from_value(unwrap_data_envelope(data)).map_err(|e| SecretError::Backend {
            message: format!("parse item: {}", e),
        })
    }

    async fn find_by_name(&self, name: &str) -> Result<BwCipher, SecretError> {
        let data = self
            .run_with_session(vec!["list", "items", "--search", name], None)
            .await?;
        let items: Vec<BwCipher> =
            serde_json::from_value(unwrap_data_envelope(data)).map_err(|e| SecretError::Backend {
                message: format!("parse list: {}", e),
            })?;
        let exact: Vec<BwCipher> = items
            .into_iter()
            .filter(|c| c.cipher_type == 1 && c.name.eq_ignore_ascii_case(name))
            .collect();
        match exact.len() {
            0 => Err(SecretError::NotFound),
            1 => Ok(exact.into_iter().next().unwrap()),
            _ => Err(SecretError::Ambiguous {
                candidates: exact.into_iter().map(|c| c.id).collect(),
            }),
        }
    }

    async fn resolve_folder_id(&self, name: &str) -> Result<Option<String>, SecretError> {
        if name.is_empty() {
            return Ok(None);
        }
        let data = self.run_with_session(vec!["list", "folders"], None).await?;
        let folders: Vec<serde_json::Value> =
            serde_json::from_value(unwrap_data_envelope(data)).map_err(|e| SecretError::Backend {
                message: format!("parse folders: {}", e),
            })?;
        Ok(folders
            .into_iter()
            .find(|f| {
                f.get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n.eq_ignore_ascii_case(name))
                    .unwrap_or(false)
            })
            .and_then(|f| f.get("id").and_then(|i| i.as_str()).map(String::from)))
    }

    async fn resolve_or_create_folder(&self, name: &str) -> Result<String, SecretError> {
        if let Some(id) = self.resolve_folder_id(name).await? {
            return Ok(id);
        }
        let body = json!({ "name": name });
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            body.to_string().as_bytes(),
        );
        let data = self
            .run_with_session(vec!["create", "folder", &encoded], None)
            .await?;
        let inner = unwrap_data_envelope(data);
        inner
            .get("id")
            .and_then(|i| i.as_str())
            .map(String::from)
            .ok_or_else(|| SecretError::Backend {
                message: "create folder did not return id".into(),
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BwCipher {
    id: String,
    name: String,
    #[serde(rename = "type")]
    cipher_type: i64,
    #[serde(default, rename = "folderId")]
    folder_id: Option<String>,
    #[serde(default, rename = "organizationId")]
    organization_id: Option<String>,
    #[serde(default, rename = "collectionIds")]
    collection_ids: Vec<String>,
    #[serde(default, rename = "revisionDate")]
    revision_date: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    login: Option<BwLogin>,
}

impl BwCipher {
    fn into_ref(self) -> SecretRef {
        SecretRef {
            item_id: self.id,
            name: self.name,
            folder: self.folder_id, // raw id; UI resolves to name lazily
            organization: self.organization_id,
            collection: self.collection_ids.into_iter().next(),
            last_modified: self.revision_date.and_then(parse_iso8601_to_ms),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BwLogin {
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
}

fn build_login_cipher(
    req: &SaveRequest,
    existing_id: Option<&str>,
    folder_id: Option<&str>,
) -> serde_json::Value {
    json!({
        "id": existing_id,
        "type": 1,
        "name": req.name,
        "notes": req.notes,
        "folderId": folder_id,
        "organizationId": req.organization,
        "collectionIds": req.collection.clone().map(|c| vec![c]).unwrap_or_default(),
        "favorite": false,
        "login": {
            "username": null,
            "password": req.value.expose_secret(),
            "totp": null,
            "uris": []
        }
    })
}

fn parse_iso8601_to_ms(s: String) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(&s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Scripted `BwExec`: each invocation pops the next response off a
    /// queue. Lets tests rehearse the exact sequence of `bw` calls a real
    /// flow would produce, including failure modes.
    struct ScriptedExec {
        responses: Mutex<Vec<BwResponse>>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl ScriptedExec {
        fn new(responses: Vec<BwResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn call_args(&self) -> Vec<Vec<String>> {
            self.calls.lock().clone()
        }
    }

    #[async_trait]
    impl BwExec for ScriptedExec {
        async fn run(&self, ctx: BwRunCtx<'_>) -> Result<BwResponse, String> {
            let captured: Vec<String> = ctx.args.iter().map(|s| s.to_string()).collect();
            self.calls.lock().push(captured);
            self.responses
                .lock()
                .pop()
                .ok_or_else(|| "no scripted response left".to_string())
                .map(|mut r| {
                    // Default success=true if data is non-null and field unset.
                    if !r.success && r.message.is_none() && !r.data.is_null() {
                        r.success = true;
                    }
                    r
                })
        }
    }

    fn ok_data(data: serde_json::Value) -> BwResponse {
        BwResponse {
            success: true,
            data,
            message: None,
            error_code: None,
        }
    }

    fn err(msg: &str) -> BwResponse {
        BwResponse {
            success: false,
            data: serde_json::Value::Null,
            message: Some(msg.into()),
            error_code: None,
        }
    }

    fn build_store(responses: Vec<BwResponse>) -> (Arc<BitwardenStore>, Arc<ScriptedExec>) {
        // Responses are popped from the END, so reverse for natural order.
        let mut responses = responses;
        responses.reverse();
        let exec = Arc::new(ScriptedExec::new(responses));
        let kc = Keychain::in_memory("test");
        let store = Arc::new(BitwardenStore::new(exec.clone(), kc));
        (store, exec)
    }

    fn cipher_json(id: &str, name: &str, password: &str) -> serde_json::Value {
        json!({
            "id": id,
            "name": name,
            "type": 1,
            "revisionDate": "2025-04-01T12:00:00.000Z",
            "login": { "password": password }
        })
    }

    #[test]
    fn map_bw_message_classifies_known_strings() {
        assert!(matches!(
            map_bw_message("Vault is locked."),
            SecretError::Locked
        ));
        assert!(matches!(
            map_bw_message("You are not logged in."),
            SecretError::LoggedOut
        ));
        assert!(matches!(map_bw_message("Not found."), SecretError::NotFound));
        assert!(matches!(
            map_bw_message("More than one result was found."),
            SecretError::Ambiguous { .. }
        ));
        assert!(matches!(
            map_bw_message("something exploded"),
            SecretError::Backend { .. }
        ));
    }

    #[tokio::test]
    async fn unavailable_store_short_circuits() {
        let store = BitwardenStore::new_unavailable("bw not installed");
        let s = store.status().await;
        assert!(matches!(s, StoreStatus::Unavailable { .. }));
        assert!(matches!(
            store.get("k").await,
            Err(SecretError::Locked) | Err(SecretError::Backend { .. })
        ));
    }

    #[tokio::test]
    async fn unlock_password_path_stores_session() {
        // Order: status (unauthenticated) → config server → status → login → unlock.
        let (store, exec) = build_store(vec![
            ok_data(json!({ "status": "unauthenticated" })),
            ok_data(json!({ "object": "message" })), // config server
            ok_data(json!({ "status": "unauthenticated" })),
            ok_data(json!({ "object": "message", "title": "logged in" })),
            ok_data(json!("session-abc123")),
        ]);
        store
            .unlock(UnlockCredentials {
                master_password: SecretString::from("pw"),
                two_factor_token: None,
                email: Some("u@x.com".into()),
                api_key_client_id: None,
                api_key_client_secret: None,
            })
            .await
            .unwrap();
        assert_eq!(
            store
                .session
                .read()
                .await
                .as_ref()
                .map(|s| s.expose_secret().to_string()),
            Some("session-abc123".into())
        );

        let calls = exec.call_args();
        assert_eq!(calls[0], vec!["status"]);
        assert_eq!(calls[1][0], "config");
        // Login lives at index 3.
        assert_eq!(calls[3][0], "login");
        assert!(calls[3].contains(&"--passwordenv".to_string()));
        // Master password must NEVER appear in argv.
        assert!(calls.iter().all(|c| !c.iter().any(|a| a == "pw")));
    }

    #[tokio::test]
    async fn unlock_when_already_logged_in_skips_login_step() {
        // status (locked) → config server → status (locked) → unlock.
        let (store, exec) = build_store(vec![
            ok_data(json!({ "status": "locked", "userEmail": "u@x.com" })),
            ok_data(json!({ "object": "message" })),
            ok_data(json!({ "status": "locked", "userEmail": "u@x.com" })),
            ok_data(json!("session-xyz")),
        ]);
        store
            .unlock(UnlockCredentials::with_password(SecretString::from("pw")))
            .await
            .unwrap();
        let calls = exec.call_args();
        assert_eq!(calls.len(), 4, "got {:?}", calls);
        // No login call — only status, config, status, unlock.
        assert!(!calls.iter().any(|c| c[0] == "login"));
        assert_eq!(calls[3][0], "unlock");
    }

    #[tokio::test]
    async fn unlock_api_key_flow_persists_creds() {
        let (store, _exec) = build_store(vec![
            ok_data(json!({ "status": "unauthenticated" })),
            ok_data(json!({ "object": "message" })), // config server
            ok_data(json!({ "status": "unauthenticated" })),
            ok_data(json!({ "object": "message" })), // login
            ok_data(json!("session-api-1")),         // unlock
        ]);
        store
            .unlock(UnlockCredentials {
                master_password: SecretString::from("pw"),
                two_factor_token: None,
                email: None,
                api_key_client_id: Some(SecretString::from("client-id")),
                api_key_client_secret: Some(SecretString::from("client-secret")),
            })
            .await
            .unwrap();
        // Keychain is in-memory; ensure both api creds got persisted.
        assert_eq!(
            store.keychain.get(BW_KEYRING_API_CLIENT_ID).unwrap(),
            "client-id"
        );
        assert_eq!(
            store.keychain.get(BW_KEYRING_API_CLIENT_SECRET).unwrap(),
            "client-secret"
        );
    }

    #[tokio::test]
    async fn save_creates_when_no_existing_match() {
        let (store, exec) = build_store(vec![
            ok_data(json!([])), // find_by_name → empty
            // No folder requested → no list folders call.
            ok_data(cipher_json("new-id", "openai", "sk-foo")), // create
            ok_data(serde_json::Value::Null), // sync
        ]);
        // Pre-set session so we don't need to unlock.
        *store.session.write().await = Some(SecretString::from("test-session"));

        let r = store
            .save(SaveRequest {
                name: "openai".into(),
                value: SecretString::from("sk-foo"),
                notes: None,
                folder: None,
                organization: None,
                collection: None,
                overwrite: false,
            })
            .await
            .unwrap();
        assert_eq!(r.item_id, "new-id");
        assert_eq!(r.name, "openai");

        let calls = exec.call_args();
        assert_eq!(calls[1][0], "create");
        assert_eq!(calls[1][1], "item");
        // Value must travel as base64-encoded JSON in argv (Bitwarden's
        // documented input shape) — but the raw plaintext must NOT appear.
        assert!(calls.iter().all(|c| !c.iter().any(|a| a.contains("sk-foo"))));
    }

    #[tokio::test]
    async fn save_without_overwrite_errors_when_exists() {
        let (store, _exec) = build_store(vec![ok_data(json!([
            cipher_json("existing-id", "openai", "old"),
        ]))]);
        *store.session.write().await = Some(SecretString::from("test-session"));

        let err = store
            .save(SaveRequest {
                name: "openai".into(),
                value: SecretString::from("new"),
                notes: None,
                folder: None,
                organization: None,
                collection: None,
                overwrite: false,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, SecretError::AlreadyExists));
    }

    #[tokio::test]
    async fn save_with_overwrite_calls_edit() {
        let (store, exec) = build_store(vec![
            ok_data(json!([cipher_json("existing-id", "openai", "old")])),
            ok_data(cipher_json("existing-id", "openai", "new")),
            ok_data(serde_json::Value::Null), // sync
        ]);
        *store.session.write().await = Some(SecretString::from("test-session"));

        store
            .save(SaveRequest {
                name: "openai".into(),
                value: SecretString::from("new"),
                notes: None,
                folder: None,
                organization: None,
                collection: None,
                overwrite: true,
            })
            .await
            .unwrap();

        let calls = exec.call_args();
        assert_eq!(calls[1][0], "edit");
        assert_eq!(calls[1][1], "item");
        assert_eq!(calls[1][2], "existing-id");
    }

    #[tokio::test]
    async fn get_returns_password_from_login_field() {
        let (store, _) = build_store(vec![ok_data(cipher_json(
            "abc",
            "openai",
            "sk-prod",
        ))]);
        *store.session.write().await = Some(SecretString::from("test-session"));

        let v = store.get("openai").await.unwrap();
        assert_eq!(v.value.expose_secret(), "sk-prod");
        assert_eq!(v.item_id, "abc");
        assert_eq!(v.name, "openai");
    }

    #[tokio::test]
    async fn get_locked_when_no_session() {
        let (store, _) = build_store(vec![]);
        // No session set.
        let err = store.get("openai").await.unwrap_err();
        assert!(matches!(err, SecretError::Locked));
    }

    #[tokio::test]
    async fn list_filters_to_logins_and_sorts() {
        let (store, _) = build_store(vec![ok_data(json!([
            { "id": "a", "name": "zeta",  "type": 1, "login": { "password": "x" } },
            { "id": "b", "name": "alpha", "type": 1, "login": { "password": "x" } },
            { "id": "c", "name": "card",  "type": 3 }, // card — filtered
        ]))]);
        *store.session.write().await = Some(SecretString::from("test-session"));

        let items = store.list(ListFilter::default()).await.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "alpha");
        assert_eq!(items[1].name, "zeta");
    }

    #[tokio::test]
    async fn search_passes_query_to_bw() {
        let (store, exec) = build_store(vec![ok_data(json!([
            { "id": "a", "name": "openai-prod", "type": 1, "login": { "password": "x" } }
        ]))]);
        *store.session.write().await = Some(SecretString::from("test-session"));

        let items = store.search("openai").await.unwrap();
        assert_eq!(items.len(), 1);
        let calls = exec.call_args();
        assert_eq!(calls[0], vec!["list", "items", "--search", "openai"]);
    }

    #[tokio::test]
    async fn delete_finds_then_deletes() {
        let (store, exec) = build_store(vec![
            ok_data(cipher_json("doomed", "old", "v")),
            ok_data(json!({ "ok": true })),
            ok_data(serde_json::Value::Null), // sync
        ]);
        *store.session.write().await = Some(SecretString::from("test-session"));

        store.delete("old").await.unwrap();
        let calls = exec.call_args();
        assert_eq!(calls[1][0], "delete");
        assert_eq!(calls[1][1], "item");
        assert_eq!(calls[1][2], "doomed");
        assert!(calls[1].contains(&"--permanent".to_string()));
    }

    #[tokio::test]
    async fn vault_locked_propagates_through_data_call() {
        let (store, _) = build_store(vec![err("Vault is locked.")]);
        *store.session.write().await = Some(SecretString::from("test-session"));
        let err = store.get("anything").await.unwrap_err();
        assert!(matches!(err, SecretError::Locked));
    }

    #[test]
    fn unwrap_data_envelope_handles_known_shapes() {
        // {object: "list", data: [...]}  → returns the array
        let listed = json!({ "object": "list", "data": [{"id": "1"}] });
        let inner = unwrap_data_envelope(listed);
        assert!(inner.is_array());
        assert_eq!(inner[0]["id"], "1");

        // {object: "template", template: {status: "..."}}  → returns the object
        let wrapped_status = json!({ "object": "template", "template": {"status": "locked"} });
        let inner = unwrap_data_envelope(wrapped_status);
        assert_eq!(inner["status"], "locked");

        // Flat (legacy) → returned as-is
        let flat = json!({ "status": "locked" });
        let inner = unwrap_data_envelope(flat);
        assert_eq!(inner["status"], "locked");
    }

    #[test]
    fn is_valid_http_url_accepts_known_shapes() {
        assert!(is_valid_http_url("https://vault.bitwarden.com"));
        assert!(is_valid_http_url("http://vaultwarden.local:8080"));
        assert!(is_valid_http_url("https://vault.example.org/path"));
    }

    #[test]
    fn is_valid_http_url_rejects_garbage() {
        assert!(!is_valid_http_url(""));
        assert!(!is_valid_http_url("ftp://vault.example.com"));
        assert!(!is_valid_http_url("javascript:alert(1)"));
        assert!(!is_valid_http_url("https://"));
        assert!(!is_valid_http_url("vault.example.com"));
    }

    #[tokio::test]
    async fn set_server_url_returns_true_on_change_only() {
        let store = BitwardenStore::new(Arc::new(NoopBwExec), Keychain::in_memory("t"));
        assert!(store.set_server_url("https://example.com".into()));
        assert!(!store.set_server_url("https://example.com".into()));
        assert!(store.set_server_url("https://other.com".into()));
        assert_eq!(store.server_url(), "https://other.com");
    }

    #[tokio::test]
    async fn unlock_runs_config_server_before_login() {
        let (store, exec) = build_store(vec![
            ok_data(json!({ "status": "unauthenticated" })),
            ok_data(json!({ "object": "message" })), // config server
            ok_data(json!({ "status": "unauthenticated" })),
            ok_data(json!({ "object": "message" })), // login
            ok_data(json!("session-server")),
        ]);
        store.set_server_url("https://vault.example.com".into());
        store
            .unlock(UnlockCredentials {
                master_password: SecretString::from("pw"),
                two_factor_token: None,
                email: Some("u@x.com".into()),
                api_key_client_id: None,
                api_key_client_secret: None,
            })
            .await
            .unwrap();
        let calls = exec.call_args();
        assert_eq!(calls[1][0], "config");
        assert_eq!(calls[1][1], "server");
        assert_eq!(calls[1][2], "https://vault.example.com");
    }

    #[tokio::test]
    async fn unlock_extracts_token_from_modern_wrapper() {
        // status (locked) → config server → status (locked) → unlock returns wrapper
        let (store, _) = build_store(vec![
            ok_data(json!({ "status": "locked", "userEmail": "u@x.com" })),
            ok_data(json!({ "object": "message" })), // config server
            ok_data(json!({ "status": "locked", "userEmail": "u@x.com" })),
            ok_data(json!({ "object": "unlock", "raw": "session-from-wrapper" })),
        ]);
        store
            .unlock(UnlockCredentials::with_password(SecretString::from("pw")))
            .await
            .unwrap();
        assert_eq!(
            store.session.read().await.as_ref().map(|s| s.expose_secret().to_string()),
            Some("session-from-wrapper".into())
        );
    }

    #[tokio::test]
    async fn unlock_extracts_token_from_template_wrapper() {
        let (store, _) = build_store(vec![
            ok_data(json!({ "status": "locked", "userEmail": "u@x.com" })),
            ok_data(json!({ "object": "message" })),
            ok_data(json!({ "status": "locked", "userEmail": "u@x.com" })),
            ok_data(json!({ "template": { "raw": "session-from-template" } })),
        ]);
        store
            .unlock(UnlockCredentials::with_password(SecretString::from("pw")))
            .await
            .unwrap();
        assert_eq!(
            store.session.read().await.as_ref().map(|s| s.expose_secret().to_string()),
            Some("session-from-template".into())
        );
    }

    #[tokio::test]
    async fn login_already_logged_in_is_treated_as_success() {
        // Status reports unauthenticated → triggers login → bw returns
        // "already logged in" → we should soft-succeed and proceed to unlock.
        let (store, _) = build_store(vec![
            ok_data(json!({ "status": "unauthenticated" })),
            ok_data(json!({ "object": "message" })),
            ok_data(json!({ "status": "unauthenticated" })),
            err("You are already logged in as u@x.com."),
            ok_data(json!("session-after-already")),
        ]);
        store
            .unlock(UnlockCredentials {
                master_password: SecretString::from("pw"),
                two_factor_token: None,
                email: Some("u@x.com".into()),
                api_key_client_id: None,
                api_key_client_secret: None,
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn unlock_forces_logout_when_server_changed() {
        // status (logged in to old) → logout → config server (new) → status (logged out) → login → unlock
        let (store, exec) = build_store(vec![
            ok_data(json!({ "status": "locked", "serverUrl": "https://old.example" })),
            ok_data(json!({ "object": "message" })), // logout
            ok_data(json!({ "object": "message" })), // config server
            ok_data(json!({ "status": "unauthenticated" })),
            ok_data(json!({ "object": "message" })), // login
            ok_data(json!("session-after-switch")),
        ]);
        store.set_server_url("https://new.example".into());
        store
            .unlock(UnlockCredentials {
                master_password: SecretString::from("pw"),
                two_factor_token: None,
                email: Some("u@x.com".into()),
                api_key_client_id: None,
                api_key_client_secret: None,
            })
            .await
            .unwrap();
        let calls = exec.call_args();
        // logout call appears before config server.
        let pos_logout = calls.iter().position(|c| c[0] == "logout").unwrap();
        let pos_config = calls.iter().position(|c| c[0] == "config").unwrap();
        assert!(pos_logout < pos_config, "logout must precede config server");
    }

    #[tokio::test]
    async fn unlock_rejects_invalid_server_url() {
        let store = BitwardenStore::new(Arc::new(NoopBwExec), Keychain::in_memory("t"));
        store.set_server_url("ftp://bad".into());
        let err = store
            .unlock(UnlockCredentials::with_password(SecretString::from("pw")))
            .await
            .unwrap_err();
        assert!(matches!(err, SecretError::BadRequest { .. }));
    }

    #[tokio::test]
    async fn bw_status_unwraps_template_envelope() {
        // Modern bw wraps the real payload in {object, template, ...}.
        let (store, _) = build_store(vec![ok_data(json!({
            "object": "template",
            "template": {
                "status": "unauthenticated",
                "serverUrl": "https://vault.example.com"
            }
        }))]);
        let s = store.bw_status().await.unwrap();
        assert_eq!(s.status, "unauthenticated");
        assert_eq!(s.server_url.as_deref(), Some("https://vault.example.com"));
    }

    #[tokio::test]
    async fn bw_status_falls_back_to_legacy_flat_shape() {
        // Older bw returned status fields at the top level.
        let (store, _) = build_store(vec![ok_data(json!({
            "status": "locked",
            "userEmail": "u@x.com"
        }))]);
        let s = store.bw_status().await.unwrap();
        assert_eq!(s.status, "locked");
        assert_eq!(s.user_email.as_deref(), Some("u@x.com"));
    }

    #[tokio::test]
    async fn status_includes_server_endpoint() {
        let (store, _exec) = build_store(vec![
            ok_data(json!({ "status": "locked", "userEmail": "u@x.com",
                            "serverUrl": "https://self.host" })),
        ]);
        store.set_server_url("https://self.host".into());
        match store.status().await {
            StoreStatus::Locked { endpoint } => {
                assert_eq!(endpoint.as_deref(), Some("https://self.host"));
            }
            other => panic!("expected Locked with endpoint, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn logout_clears_session_and_keyring() {
        let (store, _) = build_store(vec![ok_data(json!({ "ok": true }))]);
        *store.session.write().await = Some(SecretString::from("token"));
        store.keychain.set(BW_KEYRING_SESSION, "token").unwrap();
        store
            .keychain
            .set(BW_KEYRING_API_CLIENT_ID, "id")
            .unwrap();

        store.logout().await.unwrap();
        assert!(store.session.read().await.is_none());
        assert!(matches!(
            store.keychain.get(BW_KEYRING_SESSION),
            Err(KeychainError::NotFound)
        ));
        assert!(matches!(
            store.keychain.get(BW_KEYRING_API_CLIENT_ID),
            Err(KeychainError::NotFound)
        ));
    }
}
