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
/// Persisted master password for auto-unlock after daemon restart or
/// session expiry. Lives in the same keychain as the session token (file
/// in dev, OS keychain in release).
pub(super) const BW_KEYRING_MASTER_PW: &str = "master_password";
pub(super) const BW_KEYRING_LOGIN_EMAIL: &str = "login_email";

/// Default Bitwarden server URL — public hosted vault.
pub const DEFAULT_BW_SERVER: &str = "https://vault.bitwarden.com";

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
