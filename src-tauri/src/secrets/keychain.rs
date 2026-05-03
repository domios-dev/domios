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
    Os {
        service: String,
    },
    /// Pure in-memory — used in tests, when the OS keychain is unavailable,
    /// or when the `secrets-keyring` feature is disabled at compile time.
    InMemory {
        reason: String,
    },
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

    #[cfg(feature = "secrets-keyring")]
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
                    "[secrets/keychain] OS keychain unavailable ({}): falling \
                     back to in-memory (creds will not persist across daemon restarts)",
                    e
                );
                Self {
                    backend: Backend::InMemory { reason: e.to_string() },
                    shadow,
                    available: false,
                }
            }
        }
    }

    #[cfg(not(feature = "secrets-keyring"))]
    fn new_inner(_service: &str) -> Self {
        Self {
            backend: Backend::InMemory {
                reason: "built without `secrets-keyring` feature".into(),
            },
            shadow: Arc::new(Mutex::new(std::collections::HashMap::new())),
            available: false,
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
        }
    }

    pub fn set(&self, account: &str, value: &str) -> Result<(), KeychainError> {
        self.shadow
            .lock()
            .insert(account.to_string(), value.to_string());
        match &self.backend {
            #[cfg(feature = "secrets-keyring")]
            Backend::Os { service } => {
                let entry =
                    keyring::Entry::new(service, account).map_err(map_kr_err)?;
                entry.set_password(value).map_err(map_kr_err)
            }
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
                let entry =
                    keyring::Entry::new(service, account).map_err(map_kr_err)?;
                let v = entry.get_password().map_err(map_kr_err)?;
                self.shadow
                    .lock()
                    .insert(account.to_string(), v.clone());
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
                let entry =
                    keyring::Entry::new(service, account).map_err(map_kr_err)?;
                match entry.delete_credential() {
                    Ok(()) => Ok(()),
                    Err(keyring::Error::NoEntry) => Ok(()),
                    Err(e) => Err(map_kr_err(e)),
                }
            }
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
