//! File-backed secret persistence. Kept behind the historical `SecretFile` type
//! name so callers do not know whether the bytes are stored in an OS vault or
//! in Domios's own local file. The production backend is intentionally a
//! single `~/.domios/.secrets` JSON file with 0600 permissions; OS keychains
//! are avoided because signed macOS app updates can re-prompt on startup.

use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;

#[derive(Debug, Clone)]
pub enum SecretFileError {
    Unavailable(String),
    NotFound,
    Busy,
    Other(String),
}

impl std::fmt::Display for SecretFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecretFileError::Unavailable(reason) => write!(f, "keychain unavailable: {}", reason),
            SecretFileError::NotFound => write!(f, "keychain entry not found"),
            SecretFileError::Busy => write!(f, "secret store is busy"),
            SecretFileError::Other(msg) => write!(f, "keychain error: {}", msg),
        }
    }
}

impl std::error::Error for SecretFileError {}

/// In-memory shadow of stored values. Used for transient storage and as a
/// write-through cache for file-backed entries.
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
        Self::new_at(codeterm_paths::data_dir(), service)
    }

    pub fn new_at(data_root: impl AsRef<Path>, service: &str) -> Self {
        let dir = data_root.as_ref();
        if let Err(e) = std::fs::create_dir_all(dir) {
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
        let _lock = self.lock_backend()?;
        let mut shadow = self.shadow.lock();
        if let Backend::File { path, service } = &self.backend {
            write_locked_file_entry(path, &storage_account(service, account), Some(value))?;
        }
        shadow.insert(account.to_string(), value.to_string());
        Ok(())
    }

    pub fn get(&self, account: &str) -> Result<String, SecretFileError> {
        self.get_with_reader(account, read_file_entry)
    }

    fn get_with_reader(
        &self,
        account: &str,
        read: impl FnOnce(&Path, &str) -> Result<String, SecretFileError>,
    ) -> Result<String, SecretFileError> {
        let mut shadow = self.shadow.lock();
        if let Some(value) = shadow.get(account).cloned() {
            return Ok(value);
        }
        match &self.backend {
            Backend::File { path, service } => {
                let value = read(path, &storage_account(service, account))?;
                shadow.insert(account.to_string(), value.clone());
                Ok(value)
            }
            Backend::InMemory { .. } => Err(SecretFileError::NotFound),
        }
    }

    pub fn has(&self, account: &str) -> Result<bool, SecretFileError> {
        if self.shadow.lock().contains_key(account) {
            return Ok(true);
        }
        match &self.backend {
            Backend::File { path, service } => {
                has_file_entry(path, &storage_account(service, account))
            }
            Backend::InMemory { .. } => Ok(self.shadow.lock().contains_key(account)),
        }
    }

    pub fn delete(&self, account: &str) -> Result<(), SecretFileError> {
        let _lock = self.lock_backend()?;
        let mut shadow = self.shadow.lock();
        if let Backend::File { path, service } = &self.backend {
            write_locked_file_entry(path, &storage_account(service, account), None)?;
        }
        shadow.remove(account);
        Ok(())
    }

    fn lock_backend(&self) -> Result<Option<std::fs::File>, SecretFileError> {
        match &self.backend {
            Backend::File { path, .. } => lock_store(path).map(Some),
            Backend::InMemory { .. } => Ok(None),
        }
    }
}

pub fn plugin_secret_bucket(data_root: impl AsRef<Path>, plugin_id: &str) -> SecretFile {
    SecretFile::new_at(data_root, &format!("codeterm.plugin.{plugin_id}"))
}

/// Read one account from a file-backed keychain. Each service maps to a
/// JSON object `{<account>: <value>, ...}`.
fn read_file_entry(path: &std::path::Path, account: &str) -> Result<String, SecretFileError> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(SecretFileError::NotFound)
        }
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

fn has_file_entry(path: &std::path::Path, account: &str) -> Result<bool, SecretFileError> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            return Err(SecretFileError::Other(format!(
                "read {}: {}",
                path.display(),
                e
            )))
        }
    };
    if raw.trim().is_empty() {
        return Ok(false);
    }
    let map: std::collections::HashMap<String, String> = serde_json::from_str(&raw)
        .map_err(|e| SecretFileError::Other(format!("parse {}: {}", path.display(), e)))?;
    Ok(map.contains_key(account))
}

/// Write or remove an account in a file-backed keychain. Atomic rename
/// avoids corruption if the daemon dies mid-write. Permissions: 0600 on
/// Unix so other users cannot read the secret material.
#[cfg(test)]
fn write_file_entry(
    path: &std::path::Path,
    account: &str,
    value: Option<&str>,
) -> Result<(), SecretFileError> {
    let _lock = lock_store(path)?;
    write_locked_file_entry(path, account, value)
}

fn lock_store(path: &Path) -> Result<std::fs::File, SecretFileError> {
    let lock = open_store_lock(path)?;
    lock.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => SecretFileError::Busy,
        std::fs::TryLockError::Error(error) => {
            SecretFileError::Other(format!("lock secret store: {error}"))
        }
    })?;
    Ok(lock)
}

fn open_store_lock(path: &Path) -> Result<std::fs::File, SecretFileError> {
    let lock_path = path.with_file_name(".secrets.lock");
    crate::private_file::open_lock(&lock_path)
        .map_err(|error| SecretFileError::Other(format!("open secret store lock: {error}")))
}

fn write_locked_file_entry(
    path: &Path,
    account: &str,
    value: Option<&str>,
) -> Result<(), SecretFileError> {
    // Only a missing or empty file starts a new map; an unreadable one must
    // never be replaced by a map holding just this account.
    let mut map: std::collections::HashMap<String, String> = match std::fs::read_to_string(path) {
        Ok(s) if !s.trim().is_empty() => serde_json::from_str(&s)
            .map_err(|e| SecretFileError::Other(format!("parse {}: {}", path.display(), e)))?,
        Ok(_) => std::collections::HashMap::new(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::collections::HashMap::new(),
        Err(e) => {
            return Err(SecretFileError::Other(format!(
                "read {}: {}",
                path.display(),
                e
            )))
        }
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
    crate::private_file::write_atomic(path, body.as_bytes())
        .map_err(|e| SecretFileError::Other(format!("write secret store: {e}")))
}

fn storage_account(service: &str, account: &str) -> String {
    format!("{}:{}", sanitize_service(service), account)
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
mod tests;
