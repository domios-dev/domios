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

mod cipher;
mod exec;
mod secret_store;
mod store;
mod subprocess;

#[cfg(test)]
mod tests;

pub use exec::{map_bw_message, BwExec, BwResponse, BwRunCtx};
pub use store::{is_valid_http_url, BitwardenStore};
pub use subprocess::SubprocessBwExec;

use super::factory::SecretBackendFactory;
use super::SecretStore;

pub(super) const BW_KEYRING_SERVICE: &str = "codeterm.secrets.bitwarden";
pub(super) const BW_KEYRING_SESSION: &str = "session";
pub(super) const BW_KEYRING_API_CLIENT_ID: &str = "apikey:client_id";
pub(super) const BW_KEYRING_API_CLIENT_SECRET: &str = "apikey:client_secret";
pub(super) const BW_KEYRING_LOGIN_EMAIL: &str = "login_email";

/// Default Bitwarden server URL — public hosted vault.
pub const DEFAULT_BW_SERVER: &str = "https://vault.bitwarden.com";

/// Plain-file master password — `~/.codeterm/secrets/bitwarden-master.env`.
/// Read JIT on every locked operation so the agent never sees unlock UX.
pub(super) fn master_password_file_path() -> Option<std::path::PathBuf> {
    Some(
        crate::paths::data_dir()
            .join("secrets")
            .join("bitwarden-master.env"),
    )
}

/// Read `BW_MASTER_PASSWORD=...` from the env-style file. Returns None on
/// missing file, parse fail, or empty value.
pub(super) fn read_master_password_file(path: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = line.split_once('=')?;
        if k.trim() == "BW_MASTER_PASSWORD" {
            let v = v.trim().trim_matches('"').trim_matches('\'');
            return if v.is_empty() {
                None
            } else {
                Some(v.to_string())
            };
        }
    }
    None
}

/// Persist master password to `~/.codeterm/secrets/bitwarden-master.env` with
/// 0600 permissions. Best-effort: returns Err on filesystem error but doesn't
/// poison the unlock — auto-unlock just won't survive restart.
pub fn write_master_password_file(password: &str) -> Result<std::path::PathBuf, String> {
    let path = master_password_file_path().ok_or("data dir unavailable")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {}", e))?;
    }
    let body = format!("BW_MASTER_PASSWORD={}\n", password);
    std::fs::write(&path, body).map_err(|e| format!("write file: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(path)
}

pub fn delete_master_password_file() {
    if let Some(p) = master_password_file_path() {
        let _ = std::fs::remove_file(p);
    }
}

pub fn master_password_file_exists() -> bool {
    master_password_file_path()
        .map(|p| p.exists())
        .unwrap_or(false)
}

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
        Box::new(BitwardenStore::detect_and_build_with_server(server))
    }
}
