//! Pluggable secret-store layer.
//!
//! Defines `SecretStore` — a backend-agnostic trait for save / get / search /
//! list / delete of named secrets — plus the value/error types every adapter
//! shares. Concrete adapters live in sibling modules: `mock` (in-memory, used
//! for tests and dev), and (Phase 2) `bitwarden`.
//!
//! Wiring: `ServiceContext.secrets` holds an `Arc<RwLock<Box<dyn SecretStore>>>`.
//! REST + Socket + Tauri IPC + CLI all reach the active backend through that
//! single handle. Switching backends in Settings swaps the box atomically.

pub mod api;
pub mod capability;
pub mod factory;
pub mod js_backend;
pub mod legacy;
pub mod local;
pub mod mock;
pub mod secret_file;

pub use capability::{
    check_persist_consent, master_password_saved, plugin_secret_bucket, StoreCapabilities,
    AUTO_UNLOCK_CREDENTIAL_KEY,
};
pub use factory::{factory as get_factory, list as list_factories, SecretBackendFactory};
pub use legacy::{purge_legacy_master_password_file, LegacyPurge};

use std::fmt;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use zeroize::Zeroize;

/// In-process wrapper for any byte sequence that must not appear in logs,
/// `Debug` output, or panic backtraces. Always overwrites its bytes on drop.
///
/// Construct with `SecretString::from(String)`. Read with `expose_secret()` —
/// the call site is the audit point. Cloning copies the bytes; treat each
/// clone as a separate erasure responsibility.
#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    pub fn into_inner(mut self) -> String {
        std::mem::take(&mut self.0)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretString(len={})", self.0.len())
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED:{} chars]", self.0.len())
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Status of the active backend — drives the Settings UI badge and dictates
/// which CLI commands can succeed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StoreStatus {
    /// Backend selected but no creds yet (e.g. user hasn't logged into Bitwarden).
    LoggedOut {
        /// Endpoint the backend will talk to once unlocked. Lets the
        /// Bitwarden login UI confirm "you'll log in to <URL>".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        endpoint: Option<String>,
    },
    /// Creds present but the in-process session is sealed.
    Locked {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        endpoint: Option<String>,
    },
    /// Ready to read / write.
    Unlocked {
        /// Best-effort identity (email for Bitwarden, hostname for keychain, etc.).
        user: Option<String>,
        /// Backend cannot persist creds across restarts (e.g. Linux without libsecret).
        #[serde(default)]
        transient: bool,
        /// Where the backend is connected — Bitwarden server URL,
        /// keychain service, etc. Lets the user confirm "I'm signed in to
        /// the right vault" from Settings.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        endpoint: Option<String>,
    },
    /// Backend is selected but unusable — e.g. `bw` not on PATH.
    Unavailable { reason: String },
}

impl StoreStatus {
    pub fn is_unlocked(&self) -> bool {
        matches!(self, StoreStatus::Unlocked { .. })
    }
}

/// Credentials passed to `unlock`. Concrete backends pick what they need.
pub struct UnlockCredentials {
    pub master_password: SecretString,
    pub two_factor_token: Option<SecretString>,
    pub email: Option<String>,
    /// Bitwarden API-key flow.
    pub api_key_client_id: Option<SecretString>,
    pub api_key_client_secret: Option<SecretString>,
    /// The caller explicitly asked the backend to remember this credential for
    /// headless unlock. Default false: an ordinary sign-in never grants it.
    pub persist_for_auto_unlock: bool,
}

impl UnlockCredentials {
    pub fn with_password(password: SecretString) -> Self {
        Self {
            master_password: password,
            two_factor_token: None,
            email: None,
            api_key_client_id: None,
            api_key_client_secret: None,
            persist_for_auto_unlock: false,
        }
    }
}

/// Inputs for `save`. The value lives in `SecretString` so it cannot leak
/// into a `Debug` log line by accident.
pub struct SaveRequest {
    pub name: String,
    pub value: SecretString,
    pub notes: Option<String>,
    pub folder: Option<String>,
    pub organization: Option<String>,
    pub collection: Option<String>,
    /// `false` (default) returns `SecretError::AlreadyExists` if a secret with
    /// the same name is already present.
    pub overwrite: bool,
}

/// Optional filters for `list`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListFilter {
    pub folder: Option<String>,
    pub organization: Option<String>,
    pub collection: Option<String>,
    pub limit: Option<usize>,
}

/// Metadata-only descriptor returned from `list` / `search` / `save`.
/// Crucially, this contains no value bytes — never serialize a `SecretValue`
/// to the search endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretRef {
    pub item_id: String,
    pub name: String,
    pub folder: Option<String>,
    pub organization: Option<String>,
    pub collection: Option<String>,
    /// Unix milliseconds.
    pub last_modified: Option<i64>,
}

/// Returned from `get` — includes the actual plaintext value. Only the
/// reveal endpoint (`POST /api/secrets/items/{name}/value`) and the explicit
/// CLI `mem secret get` response should produce one of these.
#[derive(Debug)]
pub struct SecretValue {
    pub item_id: String,
    pub name: String,
    pub value: SecretString,
    pub notes: Option<String>,
}

/// Org-scope descriptor for the Settings picker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationRef {
    pub id: String,
    pub name: String,
}

/// Collection-scope descriptor for the Settings picker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionRef {
    pub id: String,
    pub name: String,
    pub organization_id: Option<String>,
}

/// Errors a backend can surface. Adapters map their native error vocabulary
/// to these variants so the UI / CLI behaves consistently.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SecretError {
    /// Backend selected but no creds (e.g. Bitwarden user not logged in).
    LoggedOut,
    /// Vault is sealed; caller should re-`unlock`.
    Locked,
    /// No item matched the name / id.
    NotFound,
    /// Multiple items matched a name lookup; backend can't choose.
    Ambiguous { candidates: Vec<String> },
    /// `save` with `overwrite: false` and the name already exists.
    AlreadyExists,
    /// Caller passed bad input (empty name, empty value when required, …).
    BadRequest { message: String },
    /// Backend itself is unavailable — bw not on PATH, network down, etc.
    Backend { message: String },
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretError::LoggedOut => write!(f, "secret store: logged out"),
            SecretError::Locked => write!(f, "secret store: vault is locked"),
            SecretError::NotFound => write!(f, "secret store: not found"),
            SecretError::Ambiguous { candidates } => write!(
                f,
                "secret store: ambiguous match ({} candidates)",
                candidates.len()
            ),
            SecretError::AlreadyExists => write!(f, "secret store: already exists"),
            SecretError::BadRequest { message } => write!(f, "secret store: {}", message),
            SecretError::Backend { message } => write!(f, "secret store backend: {}", message),
        }
    }
}

impl std::error::Error for SecretError {}

/// Backend interface. Adapters implement this trait; everything else in
/// CodeTerm talks to `dyn SecretStore`.
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// Stable backend id — `"mock"`, `"bitwarden"`, `"keychain"`, …. An
    /// identifier for display and storage only; never branch behaviour on it.
    fn id(&self) -> &'static str;

    /// What this backend declares it can do. Core asks this instead of asking
    /// which backend is active.
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities::NONE
    }

    /// Whether the credential a headless unlock needs is already present.
    /// Default: the host-owned plugin bucket for this backend's id — the same
    /// location `host.secretSet` writes from inside a plugin. Only meaningful
    /// when `capabilities().auto_unlock` is declared.
    fn auto_unlock_armed(&self) -> bool {
        capability::plugin_secret_bucket(self.id())
            .get(capability::AUTO_UNLOCK_CREDENTIAL_KEY)
            .is_ok()
    }

    async fn status(&self) -> StoreStatus;

    /// Try to unlock with the given creds. Returns `Ok` once the vault is
    /// usable; the next `status()` call should return `Unlocked`.
    async fn unlock(&self, creds: UnlockCredentials) -> Result<(), SecretError>;

    /// Drop the in-process session. Persisted creds (e.g. session token in
    /// `.secrets`) are kept so the user can `unlock` again without retyping.
    async fn lock(&self) -> Result<(), SecretError>;

    /// Drop the session AND any persisted creds. Equivalent to "sign out".
    async fn logout(&self) -> Result<(), SecretError>;

    async fn save(&self, req: SaveRequest) -> Result<SecretRef, SecretError>;
    async fn get(&self, name_or_id: &str) -> Result<SecretValue, SecretError>;
    async fn list(&self, filter: ListFilter) -> Result<Vec<SecretRef>, SecretError>;
    async fn search(&self, query: &str) -> Result<Vec<SecretRef>, SecretError>;
    async fn delete(&self, name_or_id: &str) -> Result<(), SecretError>;

    /// Metadata-only existence check. Backends should override with a path
    /// that does NOT decrypt the value — UI status polling must not trigger
    /// vault prompts, network round-trips, or "secret accessed" audit logs.
    async fn has(&self, name_or_id: &str) -> Result<bool, SecretError> {
        match self.get(name_or_id).await {
            Ok(_) => Ok(true),
            Err(SecretError::NotFound) => Ok(false),
            Err(e) => Err(e),
        }
    }

    async fn organizations(&self) -> Result<Vec<OrganizationRef>, SecretError> {
        Ok(Vec::new())
    }

    async fn collections(
        &self,
        _organization_id: Option<&str>,
    ) -> Result<Vec<CollectionRef>, SecretError> {
        Ok(Vec::new())
    }

    /// Pull the latest state from the remote vault. Useful after changes
    /// made via another client (Bitwarden web, mobile app, another
    /// CodeTerm machine). Default: no-op for backends with no remote
    /// (Local OS-keychain, Mock).
    async fn sync(&self) -> Result<(), SecretError> {
        Ok(())
    }

    /// One-time async work after construction: e.g. Bitwarden tries to
    /// auto-unlock from a stored master password so the user does not
    /// have to re-type after every daemon restart. Default: no-op.
    /// Returning an error is non-fatal (the store stays in its current
    /// state) but the caller can surface it for UX feedback.
    async fn init(&self) -> Result<(), SecretError> {
        Ok(())
    }
}

/// Shared handle held in `ServiceContext.secrets`. The `RwLock` lets us swap
/// the active store atomically when the user changes backend in Settings. It is
/// an `Arc<dyn SecretStore>` (not `Box`) so a plugin-contributed instance from
/// the [`SecretRegistry`] can occupy the slot without being rebuilt (gap G8).
pub type SecretStoreState = std::sync::Arc<RwLock<std::sync::Arc<dyn SecretStore>>>;

/// Runtime registry of plugin-contributed secret backends, keyed by id. A
/// plugin (e.g. Bitwarden) registers a ready instance here on activate;
/// `build_active_store` prefers it over the built-in factory.
pub type SecretRegistry = codeterm_plugin::capability::CapabilityRegistry<dyn SecretStore>;

/// Build the active backend from a full config snapshot. Prefers a plugin
/// instance registered under `cfg.backend`; otherwise builds a built-in via the
/// factory (which extracts its own typed config slice).
pub fn build_active_store(
    cfg: &crate::config::schema::SecretsConfig,
    registry: &SecretRegistry,
) -> std::sync::Arc<dyn SecretStore> {
    if let Some(instance) = registry.get(&cfg.backend) {
        return instance;
    }
    let provider_cfg = provider_config_for(cfg);
    factory::build_active(&cfg.backend, &provider_cfg)
}

fn provider_config_for(
    cfg: &crate::config::schema::SecretsConfig,
) -> std::collections::HashMap<String, serde_json::Value> {
    let mut map = std::collections::HashMap::new();
    map.insert(
        "bitwarden".into(),
        serde_json::to_value(&cfg.bitwarden).unwrap_or(serde_json::Value::Null),
    );
    map
}

/// Build the initial `SecretStoreState` from current config + plugin registry.
pub fn new_state(
    cfg: &crate::config::schema::SecretsConfig,
    registry: &SecretRegistry,
) -> SecretStoreState {
    std::sync::Arc::new(RwLock::new(build_active_store(cfg, registry)))
}

/// Validate an http(s) URL (host present) — secret backend server URL. Lives
/// here now that native Bitwarden is gone (Bitwarden is a plugin).
pub fn is_valid_http_url(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return false;
    }
    let after_scheme = trimmed
        .splitn(2, "://")
        .nth(1)
        .unwrap_or("")
        .trim_start_matches('/');
    let host = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    !host.is_empty()
}

/// One-time carry-over of a pre-existing native-Bitwarden setup into the
/// Bitwarden *plugin* namespace: server URL → `config.plugins.bitwarden.serverUrl`
/// and saved session/master/email from `codeterm.secrets.bitwarden` →
/// `codeterm.plugin.bitwarden`. Best-effort, idempotent. Returns true if config changed.
pub fn migrate_native_bitwarden_to_plugin(config: &mut crate::config::schema::AppConfig) -> bool {
    let mut changed = false;
    let native_url = config.integrations.secrets.bitwarden.server_url.clone();
    let entry = config
        .plugins
        .entry("bitwarden".to_string())
        .or_insert_with(|| serde_json::json!({}));
    let has_url = entry
        .get("serverUrl")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if !has_url && !native_url.is_empty() {
        if let Some(obj) = entry.as_object_mut() {
            obj.insert("serverUrl".into(), serde_json::json!(native_url));
            changed = true;
        }
    }
    let native = secret_file::SecretFile::new("codeterm.secrets.bitwarden");
    let plugin = capability::plugin_secret_bucket("bitwarden");
    for (from, to) in [
        ("session", "session"),
        ("master_password", "master_password"),
        ("login_email", "login_email"),
    ] {
        if plugin.get(to).is_err() {
            if let Ok(v) = native.get(from) {
                let _ = plugin.set(to, &v);
            }
        }
    }
    changed
}

/// Validate a secret name — used by every adapter to keep behaviour uniform.
/// Rules: 1..=128 chars, no leading/trailing whitespace, no NUL bytes.
pub fn validate_name(name: &str) -> Result<(), SecretError> {
    if name.is_empty() {
        return Err(SecretError::BadRequest {
            message: "name must not be empty".into(),
        });
    }
    if name.len() > 128 {
        return Err(SecretError::BadRequest {
            message: "name must be <= 128 chars".into(),
        });
    }
    if name.trim() != name {
        return Err(SecretError::BadRequest {
            message: "name must not have leading or trailing whitespace".into(),
        });
    }
    if name.contains('\0') {
        return Err(SecretError::BadRequest {
            message: "name must not contain NUL bytes".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_string_does_not_leak_in_debug() {
        let s = SecretString::from("super-secret");
        let debug = format!("{:?}", s);
        assert!(!debug.contains("super-secret"));
        assert!(debug.contains("len=12"));

        let display = format!("{}", s);
        assert!(!display.contains("super-secret"));
        assert!(display.contains("REDACTED"));
    }

    #[test]
    fn validate_name_accepts_normal_input() {
        assert!(validate_name("openai-prod").is_ok());
        assert!(validate_name("foo.bar/baz").is_ok());
    }

    #[test]
    fn validate_name_rejects_pathological_input() {
        assert!(matches!(
            validate_name(""),
            Err(SecretError::BadRequest { .. })
        ));
        assert!(matches!(
            validate_name(" leading"),
            Err(SecretError::BadRequest { .. })
        ));
        assert!(matches!(
            validate_name("trailing "),
            Err(SecretError::BadRequest { .. })
        ));
        assert!(matches!(
            validate_name("contains\0nul"),
            Err(SecretError::BadRequest { .. })
        ));
        let oversized = "a".repeat(129);
        assert!(matches!(
            validate_name(&oversized),
            Err(SecretError::BadRequest { .. })
        ));
    }

    fn cfg_with(backend: &str) -> crate::config::schema::SecretsConfig {
        crate::config::schema::SecretsConfig {
            backend: backend.to_string(),
            ..Default::default()
        }
    }

    fn empty_registry() -> SecretRegistry {
        SecretRegistry::new()
    }

    #[test]
    fn build_active_store_falls_back_to_local_for_unknown_backend() {
        assert_eq!(
            build_active_store(&cfg_with("garbage"), &empty_registry()).id(),
            "local"
        );
    }

    #[test]
    fn build_active_store_local_is_default() {
        assert_eq!(
            build_active_store(&cfg_with("local"), &empty_registry()).id(),
            "local"
        );
    }

    #[test]
    fn build_active_store_mock_still_available_for_tests() {
        assert_eq!(
            build_active_store(&cfg_with("mock"), &empty_registry()).id(),
            "mock"
        );
    }

    #[test]
    fn build_active_store_prefers_registered_plugin_instance() {
        let reg = SecretRegistry::new();
        reg.register(
            "some-plugin-store",
            std::sync::Arc::new(mock::MockStore::default()),
        );
        // A registered plugin instance wins over the native factory fallback.
        let store = build_active_store(&cfg_with("some-plugin-store"), &reg);
        assert_eq!(store.id(), mock::MockStore::default().id());
    }

    struct GetOnlyStore {
        present: bool,
        get_calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl SecretStore for GetOnlyStore {
        fn id(&self) -> &'static str {
            "get-only-test"
        }
        async fn status(&self) -> StoreStatus {
            StoreStatus::Unlocked {
                user: None,
                transient: true,
                endpoint: None,
            }
        }
        async fn unlock(&self, _: UnlockCredentials) -> Result<(), SecretError> {
            Ok(())
        }
        async fn lock(&self) -> Result<(), SecretError> {
            Ok(())
        }
        async fn logout(&self) -> Result<(), SecretError> {
            Ok(())
        }
        async fn save(&self, _: SaveRequest) -> Result<SecretRef, SecretError> {
            Err(SecretError::BadRequest {
                message: "unsupported".into(),
            })
        }
        async fn get(&self, name_or_id: &str) -> Result<SecretValue, SecretError> {
            self.get_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.present {
                Ok(SecretValue {
                    item_id: "id-1".into(),
                    name: name_or_id.into(),
                    value: SecretString::from("v"),
                    notes: None,
                })
            } else {
                Err(SecretError::NotFound)
            }
        }
        async fn list(&self, _: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
            Ok(Vec::new())
        }
        async fn search(&self, _: &str) -> Result<Vec<SecretRef>, SecretError> {
            Ok(Vec::new())
        }
        async fn delete(&self, _: &str) -> Result<(), SecretError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn default_has_impl_returns_true_for_existing_item() {
        let store = GetOnlyStore {
            present: true,
            get_calls: std::sync::atomic::AtomicUsize::new(0),
        };
        assert_eq!(store.has("anything").await.unwrap(), true);
        assert_eq!(store.get_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn default_has_impl_returns_false_for_not_found() {
        let store = GetOnlyStore {
            present: false,
            get_calls: std::sync::atomic::AtomicUsize::new(0),
        };
        assert_eq!(store.has("missing").await.unwrap(), false);
        assert_eq!(store.get_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn default_has_impl_propagates_other_errors() {
        struct LockedStore;
        #[async_trait]
        impl SecretStore for LockedStore {
            fn id(&self) -> &'static str {
                "locked-test"
            }
            async fn status(&self) -> StoreStatus {
                StoreStatus::Locked { endpoint: None }
            }
            async fn unlock(&self, _: UnlockCredentials) -> Result<(), SecretError> {
                Ok(())
            }
            async fn lock(&self) -> Result<(), SecretError> {
                Ok(())
            }
            async fn logout(&self) -> Result<(), SecretError> {
                Ok(())
            }
            async fn save(&self, _: SaveRequest) -> Result<SecretRef, SecretError> {
                Err(SecretError::Locked)
            }
            async fn get(&self, _: &str) -> Result<SecretValue, SecretError> {
                Err(SecretError::Locked)
            }
            async fn list(&self, _: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
                Err(SecretError::Locked)
            }
            async fn search(&self, _: &str) -> Result<Vec<SecretRef>, SecretError> {
                Err(SecretError::Locked)
            }
            async fn delete(&self, _: &str) -> Result<(), SecretError> {
                Err(SecretError::Locked)
            }
        }
        let store = LockedStore;
        assert!(matches!(store.has("k").await, Err(SecretError::Locked)));
    }
}
