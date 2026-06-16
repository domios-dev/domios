//! File-backed secret persistence. Kept behind the historical `SecretFile` type
//! name so callers do not know whether the bytes are stored in an OS vault or
//! in CodeTerm's own local file. The production backend is intentionally a
//! single `~/.codeterm/.secrets` JSON file with 0600 permissions; OS keychains
//! are avoided because signed macOS app updates can re-prompt on startup.

use std::sync::Arc;

use parking_lot::Mutex;

#[derive(Debug, Clone)]
pub enum SecretFileError {
    Unavailable(String),
    NotFound,
    Other(String),
}

impl std::fmt::Display for SecretFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecretFileError::Unavailable(reason) => write!(f, "keychain unavailable: {}", reason),
            SecretFileError::NotFound => write!(f, "keychain entry not found"),
            SecretFileError::Other(msg) => write!(f, "keychain error: {}", msg),
        }
    }
}

impl std::error::Error for SecretFileError {}

/// In-memory shadow of stored values. Used both as the only storage for
/// the in-memory variant and as a write-through cache for the OS-backed
/// variant (cuts duplicate keychain reads inside one daemon lifetime).
type Shadow = Arc<Mutex<std::collections::HashMap<String, String>>>;

#[derive(Clone)]
enum Backend {
    /// Plain JSON file at `<data_dir>/.secrets`, permissions 0600. Account
    /// names are namespaced by service inside the shared file.
    File {
        path: std::path::PathBuf,
        service: String,
    },
    /// Pure in-memory — used in tests when the caller has decided
    /// persistence is not wanted.
    InMemory { reason: String },
}

#[derive(Clone)]
pub struct SecretFile {
    backend: Backend,
    shadow: Shadow,
    available: bool,
}

impl SecretFile {
    /// Build the local file-backed store. This never probes or touches an OS
    /// keychain, so app startup cannot trigger macOS SecretFile prompts.
    pub fn new(service: &str) -> Self {
        Self::file_inner(service)
    }

    fn file_inner(service: &str) -> Self {
        let dir = crate::paths::data_dir();
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
        let path = dir.join(".secrets");
        migrate_legacy_file_backend(service, &path);
        Self {
            backend: Backend::File {
                path,
                service: service.to_string(),
            },
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
            Backend::File { .. } => None,
        }
    }

    pub fn set(&self, account: &str, value: &str) -> Result<(), SecretFileError> {
        self.shadow
            .lock()
            .insert(account.to_string(), value.to_string());
        match &self.backend {
            Backend::File { path, service } => {
                write_file_entry(path, &storage_account(service, account), Some(value))
            }
            Backend::InMemory { .. } => Ok(()),
        }
    }

    pub fn get(&self, account: &str) -> Result<String, SecretFileError> {
        if let Some(v) = self.shadow.lock().get(account).cloned() {
            return Ok(v);
        }
        match &self.backend {
            Backend::File { path, service } => {
                let v = read_file_entry(path, &storage_account(service, account))?;
                self.shadow.lock().insert(account.to_string(), v.clone());
                Ok(v)
            }
            Backend::InMemory { .. } => Err(SecretFileError::NotFound),
        }
    }

    pub fn delete(&self, account: &str) -> Result<(), SecretFileError> {
        self.shadow.lock().remove(account);
        match &self.backend {
            Backend::File { path, service } => {
                write_file_entry(path, &storage_account(service, account), None)
            }
            Backend::InMemory { .. } => Ok(()),
        }
    }
}

/// Read one account from a file-backed keychain. Each service maps to a
/// JSON object `{<account>: <value>, ...}`.
fn read_file_entry(path: &std::path::Path, account: &str) -> Result<String, SecretFileError> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(SecretFileError::NotFound),
        Err(e) => {
            return Err(SecretFileError::Other(format!(
                "read {}: {}",
                path.display(),
                e
            )))
        }
    };
    if raw.trim().is_empty() {
        return Err(SecretFileError::NotFound);
    }
    let map: std::collections::HashMap<String, String> = serde_json::from_str(&raw)
        .map_err(|e| SecretFileError::Other(format!("parse {}: {}", path.display(), e)))?;
    map.get(account).cloned().ok_or(SecretFileError::NotFound)
}

/// Write or remove an account in a file-backed keychain. Atomic rename
/// avoids corruption if the daemon dies mid-write. Permissions: 0600 on
/// Unix so other users cannot read the secret material.
fn write_file_entry(
    path: &std::path::Path,
    account: &str,
    value: Option<&str>,
) -> Result<(), SecretFileError> {
    let mut map: std::collections::HashMap<String, String> = match std::fs::read_to_string(path) {
        Ok(s) if !s.trim().is_empty() => serde_json::from_str(&s)
            .map_err(|e| SecretFileError::Other(format!("parse {}: {}", path.display(), e)))?,
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
        .map_err(|e| SecretFileError::Other(format!("encode keychain file: {}", e)))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body)
        .map_err(|e| SecretFileError::Other(format!("write {}: {}", tmp.display(), e)))?;
    restrict_file_permissions(&tmp);
    std::fs::rename(&tmp, path)
        .map_err(|e| SecretFileError::Other(format!("rename {}: {}", path.display(), e)))?;
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

fn storage_account(service: &str, account: &str) -> String {
    format!("{}:{}", sanitize_service(service), account)
}

fn migrate_legacy_file_backend(service: &str, new_path: &std::path::Path) {
    let old_path = crate::paths::data_dir()
        .join("keychain")
        .join(format!("{}.json", sanitize_service(service)));
    if !old_path.exists() {
        return;
    }
    let raw = match std::fs::read_to_string(&old_path) {
        Ok(s) if !s.trim().is_empty() => s,
        _ => return,
    };
    let map: std::collections::HashMap<String, String> = match serde_json::from_str(&raw) {
        Ok(map) => map,
        Err(e) => {
            log::warn!(
                "[secrets/keychain] failed to parse legacy {}: {}",
                old_path.display(),
                e
            );
            return;
        }
    };
    for (account, value) in map {
        let namespaced = storage_account(service, &account);
        if read_file_entry(new_path, &namespaced).is_err() {
            let _ = write_file_entry(new_path, &namespaced, Some(&value));
        }
    }
}

/// Make a service name safe to use as a key namespace — strip path separators
/// and the few characters Windows reserves.
fn sanitize_service(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_round_trip() {
        let kc = SecretFile::in_memory("test");
        assert!(!kc.is_available());
        assert_eq!(kc.availability_reason(), Some("test"));

        kc.set("foo", "bar").unwrap();
        assert_eq!(kc.get("foo").unwrap(), "bar");

        kc.delete("foo").unwrap();
        assert!(matches!(kc.get("foo"), Err(SecretFileError::NotFound)));

        // Delete is idempotent.
        kc.delete("foo").unwrap();
    }

    #[test]
    fn in_memory_overwrite_is_last_write_wins() {
        let kc = SecretFile::in_memory("test");
        kc.set("k", "v1").unwrap();
        kc.set("k", "v2").unwrap();
        assert_eq!(kc.get("k").unwrap(), "v2");
    }
}
