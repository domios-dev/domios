use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use zeroize::Zeroize;

pub struct ApiKey(String);

impl ApiKey {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let mut value = value.into();
        let trimmed = value.trim().to_string();
        value.zeroize();
        (!trimmed.is_empty()).then_some(Self(trimmed))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Clone for ApiKey {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Drop for ApiKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// The Domios secret store as seen by the embedder; the host adapts its
/// backend to this port.
pub trait KeyVault: Send + Sync {
    fn contains(&self, name: &str) -> Result<bool, String>;
    fn read(&self, name: &str) -> Result<Option<ApiKey>, String>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyFile {
    pub path: PathBuf,
    pub present: bool,
    pub owner_only: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyPresence {
    pub file: KeyFile,
    pub vault_name: String,
    pub in_vault: bool,
}

impl KeyPresence {
    pub fn available(&self) -> bool {
        self.file.present || self.in_vault
    }
}

#[derive(Clone)]
pub struct KeySources {
    // The owner chose a file over the slow secret store for faster activation.
    secrets_dir: PathBuf,
    vault: Option<Arc<dyn KeyVault>>,
}

impl KeySources {
    pub fn new(secrets_dir: PathBuf, vault: Option<Arc<dyn KeyVault>>) -> Self {
        Self { secrets_dir, vault }
    }

    pub fn key_file(&self, provider: &str) -> PathBuf {
        self.secrets_dir.join(format!("{provider}-embedder.key"))
    }

    pub fn vault_name(provider: &str) -> String {
        format!("{provider}-embedder")
    }

    pub fn presence(&self, provider: &str) -> KeyPresence {
        let path = self.key_file(provider);
        let present = path.is_file();
        let owner_only = present.then(|| owner_only(&path).ok()).flatten();
        let vault_name = Self::vault_name(provider);
        let in_vault = self
            .vault
            .as_ref()
            .is_some_and(|vault| vault.contains(&vault_name).unwrap_or(false));
        KeyPresence {
            file: KeyFile {
                path,
                present,
                owner_only,
            },
            vault_name,
            in_vault,
        }
    }

    /// The key file wins when both sources hold a key.
    pub fn resolve(&self, provider: &str) -> Result<ApiKey, String> {
        let path = self.key_file(provider);
        if path.is_file() {
            if matches!(owner_only(&path), Ok(false)) {
                return Err(format!(
                    "{} is not owner-only; replace it with domios mem embedder key set {provider} (key on piped stdin); manual Unix/Windows permissions: domios docs MEMORY",
                    path.display()
                ));
            }
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| format!("read {}: {e}", path.display()))?;
            return ApiKey::new(raw).ok_or_else(|| format!("{} is empty", path.display()));
        }
        let name = Self::vault_name(provider);
        match &self.vault {
            Some(vault) => vault.read(&name)?.ok_or_else(|| {
                format!("no {provider} embedder key in the key file or the secret store")
            }),
            None => Err(format!(
                "no {provider} embedder key in the key file and no secret store"
            )),
        }
    }
}

#[cfg(unix)]
fn owner_only(path: &Path) -> std::io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o077 == 0)
}

#[cfg(windows)]
fn owner_only(path: &Path) -> std::io::Result<bool> {
    crate::decision::token_store::owner_only(path)
}

#[cfg(all(not(unix), not(windows)))]
fn owner_only(_path: &Path) -> std::io::Result<bool> {
    Ok(true)
}

/// A key resolved once and cached; only an authentication failure reads the
/// sources again.
pub struct CachedKey {
    sources: KeySources,
    provider: String,
    state: Mutex<KeyState>,
}

enum KeyState {
    Ready(ApiKey),
    Missing(String),
}

impl CachedKey {
    pub fn resolve(sources: KeySources, provider: &str) -> Arc<Self> {
        let state = match sources.resolve(provider) {
            Ok(key) => KeyState::Ready(key),
            Err(reason) => KeyState::Missing(reason),
        };
        Arc::new(Self {
            sources,
            provider: provider.to_string(),
            state: Mutex::new(state),
        })
    }

    pub fn seeded(sources: KeySources, provider: &str, key: ApiKey) -> Arc<Self> {
        Arc::new(Self {
            sources,
            provider: provider.to_string(),
            state: Mutex::new(KeyState::Ready(key)),
        })
    }

    pub fn get(&self) -> Result<ApiKey, String> {
        match &*self.state.lock() {
            KeyState::Ready(key) => Ok(key.clone()),
            KeyState::Missing(reason) => Err(reason.clone()),
        }
    }

    /// Read the sources again after `rejected` failed authentication; the
    /// same key stays refused until it is replaced.
    pub fn auth_failed(&self, rejected: &ApiKey) {
        let state = match self.sources.resolve(&self.provider) {
            Ok(key) if key.expose() != rejected.expose() => KeyState::Ready(key),
            Ok(_) => KeyState::Missing("the stored key was rejected; replace it".into()),
            Err(reason) => KeyState::Missing(reason),
        };
        *self.state.lock() = state;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateRefusal {
    pub provider: String,
    pub missing_key: bool,
    pub missing_opt_in: bool,
    pub key_file: PathBuf,
    pub vault_name: String,
}

impl GateRefusal {
    pub fn message(&self) -> String {
        let mut reasons = Vec::new();
        if self.missing_key {
            reasons.push("no API key is stored for it");
        }
        if self.missing_opt_in {
            reasons.push("the remote-embedder opt-in (memory.embedder.remote_opt_in) is not set");
        }
        format!(
            "the {} embedder is remote and cannot be activated: {}",
            self.provider,
            reasons.join(" and ")
        )
    }

    pub fn hints(&self) -> Vec<String> {
        let mut hints = Vec::new();
        if self.missing_key {
            hints.push(format!(
                "store the key via piped stdin: domios mem embedder key set {} (writes {})",
                self.provider,
                self.key_file.display()
            ));
            hints.push(
                "manual fallback: domios docs MEMORY has separate Unix and Windows owner-only file recipes".into()
            );
            hints.push(format!(
                "or in the secret store: domios mem secret save --name {}   (key on stdin)",
                self.vault_name
            ));
        }
        if self.missing_opt_in {
            hints.push(
                "after reading the warning that memory text goes to a third party: domios config set memory.embedder.remote_opt_in true"
                    .into(),
            );
        }
        hints
    }
}

/// A remote provider activates only with a stored key AND the explicit opt-in.
pub fn check_remote(
    provider: &str,
    remote: bool,
    opt_in: bool,
    presence: &KeyPresence,
) -> Result<(), GateRefusal> {
    if !remote {
        return Ok(());
    }
    let missing_key = !presence.available();
    if !missing_key && opt_in {
        return Ok(());
    }
    Err(GateRefusal {
        provider: provider.to_string(),
        missing_key,
        missing_opt_in: !opt_in,
        key_file: presence.file.path.clone(),
        vault_name: presence.vault_name.clone(),
    })
}

pub const REMOTE_WARNING: &str =
    "this embedder is remote: memory text is sent to a third party to be embedded";
