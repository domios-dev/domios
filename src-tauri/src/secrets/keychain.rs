//! Thin wrapper over the `keyring` crate. Centralizes the OS-keychain
//! contact surface so the Bitwarden adapter (and future adapters) don't
//! sprinkle `keyring::Entry::new(...)` calls everywhere.
//!
//! Behavior matrix:
//! - macOS / Windows: native keychain available; `is_available()` returns true.
//! - Linux with libsecret + Secret Service: same.
//! - Linux without libsecret: `is_available()` returns false. Adapters
//!   should switch to in-process-only ("transient") mode rather than
//!   falling back to plaintext on disk.
//! - Build without the `secrets-keyring` feature: same as headless-Linux.

use std::sync::Arc;

use parking_lot::Mutex;

#[derive(Debug, Clone)]
pub enum KeychainError {
    Unavailable(String),
    NotFound,
    Other(String),
}

impl std::fmt::Display for KeychainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeychainError::Unavailable(reason) => write!(f, "keychain unavailable: {}", reason),
            KeychainError::NotFound => write!(f, "keychain entry not found"),
            KeychainError::Other(msg) => write!(f, "keychain error: {}", msg),
        }
    }
}

impl std::error::Error for KeychainError {}

/// In-memory shadow of stored values. Used both as the only storage for
/// the in-memory variant and as a write-through cache for the OS-backed
/// variant (cuts duplicate keychain reads inside one daemon lifetime).
type Shadow = Arc<Mutex<std::collections::HashMap<String, String>>>;

#[derive(Clone)]
enum Backend {
    /// OS keychain via the `keyring` crate.
    #[cfg(feature = "secrets-keyring")]
    Os { service: String },
    /// Plain JSON file at `<data_dir>/keychain/<service>.json`,
    /// permissions 0600. Used in dev builds (where every rebuild changes
    /// the binary signature and macOS prompts on every keychain access)
    /// and on platforms where the OS keychain is unavailable. The bytes
    /// are still secret-grade; the only protection is filesystem perms.
    File { path: std::path::PathBuf },
    /// Pure in-memory — used in tests when the caller has decided
    /// persistence is not wanted.
    InMemory { reason: String },
}

#[derive(Clone)]
pub struct Keychain {
    backend: Backend,
    shadow: Shadow,
    available: bool,
}

impl Keychain {
    /// Build the OS-backed keychain. Probes by writing then deleting a
    /// canary entry; if the probe fails we fall through to in-memory mode
    /// and report the underlying reason via `availability_reason()`.
    pub fn new(service: &str) -> Self {
        Self::new_inner(service)
    }

    #[cfg(all(feature = "secrets-keyring", not(debug_assertions)))]
    fn new_inner(service: &str) -> Self {
        let probe = service.to_string();
        let shadow: Shadow = Arc::new(Mutex::new(std::collections::HashMap::new()));
        match Self::probe(&probe) {
            Ok(()) => Self {
                backend: Backend::Os { service: probe },
                shadow,
                available: true,
            },
            Err(e) => {
                log::warn!(
                    "[secrets/keychain] OS keychain unavailable ({}): falling back to file storage",
                    e
                );
                Self::file_inner(service)
            }
        }
    }

    /// In dev builds we always use file storage. Each `cargo build` produces
    /// a new binary signature, which macOS treats as a brand-new app and
    /// prompts for keychain access on every read — completely unusable for
    /// iterative work. File storage at `~/.codeterm-dev/keychain/<service>.json`
    /// (mode 0600) gives the same persistence with zero prompts.
    #[cfg(all(feature = "secrets-keyring", debug_assertions))]
    fn new_inner(service: &str) -> Self {
        Self::file_inner(service)
    }

    #[cfg(not(feature = "secrets-keyring"))]
    fn new_inner(service: &str) -> Self {
        Self::file_inner(service)
    }

    fn file_inner(service: &str) -> Self {
        let dir = crate::paths::data_dir().join("keychain");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!(
                "[secrets/keychain] failed to create file backend dir {}: {} — falling back to in-memory",
                dir.display(),
                e
            );
            return Self {
                backend: Backend::InMemory {
                    reason: format!("mkdir {}: {}", dir.display(), e),
                },
                shadow: Arc::new(Mutex::new(std::collections::HashMap::new())),
                available: false,
            };
        }
        let path = dir.join(format!("{}.json", sanitize_service(service)));
        Self {
            backend: Backend::File { path },
            shadow: Arc::new(Mutex::new(std::collections::HashMap::new())),
            available: true,
        }
    }

    /// In-memory backend explicitly. Useful for tests or when the caller
    /// has decided persistence is not wanted.
    pub fn in_memory(reason: impl Into<String>) -> Self {
        Self {
            backend: Backend::InMemory {
                reason: reason.into(),
            },
            shadow: Arc::new(Mutex::new(std::collections::HashMap::new())),
            available: false,
        }
    }

    pub fn is_available(&self) -> bool {
        self.available
    }

    pub fn availability_reason(&self) -> Option<&str> {
        match &self.backend {
            Backend::InMemory { reason } => Some(reason.as_str()),
            #[cfg(feature = "secrets-keyring")]
            Backend::Os { .. } => None,
            Backend::File { .. } => None,
        }
    }

    pub fn set(&self, account: &str, value: &str) -> Result<(), KeychainError> {
        self.shadow
            .lock()
            .insert(account.to_string(), value.to_string());
        match &self.backend {
            #[cfg(feature = "secrets-keyring")]
            Backend::Os { service } => {
                let entry = keyring::Entry::new(service, account).map_err(map_kr_err)?;
                entry.set_password(value).map_err(map_kr_err)
            }
            Backend::File { path } => write_file_entry(path, account, Some(value)),
            Backend::InMemory { .. } => Ok(()),
        }
    }

    pub fn get(&self, account: &str) -> Result<String, KeychainError> {
        if let Some(v) = self.shadow.lock().get(account).cloned() {
            return Ok(v);
        }
        match &self.backend {
            #[cfg(feature = "secrets-keyring")]
            Backend::Os { service } => {
                let entry = keyring::Entry::new(service, account).map_err(map_kr_err)?;
                let v = entry.get_password().map_err(map_kr_err)?;
                self.shadow.lock().insert(account.to_string(), v.clone());
                Ok(v)
            }
            Backend::File { path } => {
                let v = read_file_entry(path, account)?;
                self.shadow.lock().insert(account.to_string(), v.clone());
                Ok(v)
            }
            Backend::InMemory { .. } => Err(KeychainError::NotFound),
        }
    }

    pub fn delete(&self, account: &str) -> Result<(), KeychainError> {
        self.shadow.lock().remove(account);
        match &self.backend {
            #[cfg(feature = "secrets-keyring")]
            Backend::Os { service } => {
                let entry = keyring::Entry::new(service, account).map_err(map_kr_err)?;
                match entry.delete_credential() {
                    Ok(()) => Ok(()),
                    Err(keyring::Error::NoEntry) => Ok(()),
                    Err(e) => Err(map_kr_err(e)),
                }
            }
            Backend::File { path } => write_file_entry(path, account, None),
            Backend::InMemory { .. } => Ok(()),
        }
    }

    #[cfg(feature = "secrets-keyring")]
    fn probe(service: &str) -> Result<(), KeychainError> {
        let canary_account = "__codeterm_probe__";
        let entry = keyring::Entry::new(service, canary_account).map_err(map_kr_err)?;
        entry.set_password("ok").map_err(map_kr_err)?;
        let _ = entry.delete_credential();
        Ok(())
    }
}

/// Read one account from a file-backed keychain. Each service maps to a
/// JSON object `{<account>: <value>, ...}`.
fn read_file_entry(path: &std::path::Path, account: &str) -> Result<String, KeychainError> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(KeychainError::NotFound),
        Err(e) => {
            return Err(KeychainError::Other(format!(
                "read {}: {}",
                path.display(),
                e
            )))
        }
    };
    if raw.trim().is_empty() {
        return Err(KeychainError::NotFound);
    }
    let map: std::collections::HashMap<String, String> = serde_json::from_str(&raw)
        .map_err(|e| KeychainError::Other(format!("parse {}: {}", path.display(), e)))?;
    map.get(account).cloned().ok_or(KeychainError::NotFound)
}

/// Write or remove an account in a file-backed keychain. Atomic rename
/// avoids corruption if the daemon dies mid-write. Permissions: 0600 on
/// Unix so other users cannot read the secret material.
fn write_file_entry(
    path: &std::path::Path,
    account: &str,
    value: Option<&str>,
) -> Result<(), KeychainError> {
    let mut map: std::collections::HashMap<String, String> = match std::fs::read_to_string(path) {
        Ok(s) if !s.trim().is_empty() => serde_json::from_str(&s)
            .map_err(|e| KeychainError::Other(format!("parse {}: {}", path.display(), e)))?,
        _ => std::collections::HashMap::new(),
    };
    match value {
        Some(v) => {
            map.insert(account.to_string(), v.to_string());
        }
        None => {
            map.remove(account);
        }
    }
    let body = serde_json::to_string(&map)
        .map_err(|e| KeychainError::Other(format!("encode keychain file: {}", e)))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body)
        .map_err(|e| KeychainError::Other(format!("write {}: {}", tmp.display(), e)))?;
    restrict_file_permissions(&tmp);
    std::fs::rename(&tmp, path)
        .map_err(|e| KeychainError::Other(format!("rename {}: {}", path.display(), e)))?;
    Ok(())
}

/// Restrict file permissions so only the current user can read the file.
/// On Unix: chmod 0600. On Windows: remove inherited ACEs via icacls and
/// grant full control to the current user only.
fn restrict_file_permissions(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(windows)]
    {
        let path_str = path.to_string_lossy();
        let username = std::env::var("USERNAME").unwrap_or_default();
        if !username.is_empty() {
            let _ = std::process::Command::new("icacls")
                .args([
                    path_str.as_ref(),
                    "/inheritance:r",
                    "/grant",
                    &format!("{}:(F)", username),
                ])
                .output();
        }
    }
}

/// Make a service name safe to use as a filename — strip path separators
/// and the few characters Windows reserves.
fn sanitize_service(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            other => other,
        })
        .collect()
}

#[cfg(feature = "secrets-keyring")]
fn map_kr_err(e: keyring::Error) -> KeychainError {
    use keyring::Error;
    match e {
        Error::NoEntry => KeychainError::NotFound,
        Error::PlatformFailure(inner) => KeychainError::Unavailable(inner.to_string()),
        Error::NoStorageAccess(inner) => KeychainError::Unavailable(inner.to_string()),
        Error::Ambiguous(_) => KeychainError::Other("ambiguous keychain match".into()),
        other => KeychainError::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_round_trip() {
        let kc = Keychain::in_memory("test");
        assert!(!kc.is_available());
        assert_eq!(kc.availability_reason(), Some("test"));

        kc.set("foo", "bar").unwrap();
        assert_eq!(kc.get("foo").unwrap(), "bar");

        kc.delete("foo").unwrap();
        assert!(matches!(kc.get("foo"), Err(KeychainError::NotFound)));

        // Delete is idempotent.
        kc.delete("foo").unwrap();
    }

    #[test]
    fn in_memory_overwrite_is_last_write_wins() {
        let kc = Keychain::in_memory("test");
        kc.set("k", "v1").unwrap();
        kc.set("k", "v2").unwrap();
        assert_eq!(kc.get("k").unwrap(), "v2");
    }
}
