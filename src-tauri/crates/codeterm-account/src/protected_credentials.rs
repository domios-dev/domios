use crate::{Error, Result, Secret};
use std::{collections::HashMap, sync::Mutex};

mod native;

pub struct ProtectedCredentials {
    vault: Box<dyn Vault>,
    memory: Mutex<HashMap<String, Secret>>,
}
impl ProtectedCredentials {
    pub fn new(namespace: String) -> Self {
        Self {
            vault: Box::new(OsVault(namespace)),
            memory: Mutex::new(HashMap::new()),
        }
    }
    pub fn session_only(&self, name: &str) -> bool {
        self.memory
            .lock()
            .is_ok_and(|memory| memory.contains_key(name))
    }
    pub fn load(&self, name: &str) -> Result<Option<Secret>> {
        if let Some(value) = self
            .memory
            .lock()
            .map_err(|_| Error::Credentials)?
            .get(name)
        {
            return Ok(Some(value.clone()));
        }
        Ok(self.vault.load(name).ok().flatten())
    }
    pub fn save(&self, name: &str, token: &Secret) -> Result<()> {
        let persisted = self.vault.save(name, token).is_ok();
        self.remember(name, token, persisted)
    }
    fn remember(&self, name: &str, token: &Secret, persisted: bool) -> Result<()> {
        let mut memory = self.memory.lock().map_err(|_| Error::Credentials)?;
        if persisted {
            memory.remove(name);
        } else {
            log::warn!("[account] OS credential store unavailable; sign-in session is kept in memory only and will not survive restart");
            memory.insert(name.to_owned(), token.clone());
        }
        Ok(())
    }
    pub fn delete(&self, name: &str) -> Result<()> {
        self.memory
            .lock()
            .map_err(|_| Error::Credentials)?
            .remove(name);
        if self.vault.load(name).ok().flatten().is_some() {
            self.vault.delete(name)?;
        }
        Ok(())
    }
}

trait Vault: Send + Sync {
    fn load(&self, name: &str) -> Result<Option<Secret>>;
    fn save(&self, name: &str, token: &Secret) -> Result<()>;
    fn delete(&self, name: &str) -> Result<()>;
}
struct OsVault(String);
impl Vault for OsVault {
    fn load(&self, name: &str) -> Result<Option<Secret>> {
        native::load(&self.0, name)
    }
    fn save(&self, name: &str, token: &Secret) -> Result<()> {
        native::save(&self.0, name, token)
    }
    fn delete(&self, name: &str) -> Result<()> {
        native::delete(&self.0, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Unavailable;
    impl Vault for Unavailable {
        fn load(&self, _: &str) -> Result<Option<Secret>> {
            Err(Error::Credentials)
        }
        fn save(&self, _: &str, _: &Secret) -> Result<()> {
            Err(Error::Credentials)
        }
        fn delete(&self, _: &str) -> Result<()> {
            Err(Error::Credentials)
        }
    }

    #[test]
    fn unavailable_vault_keeps_refresh_credential_in_memory_only() {
        let store = ProtectedCredentials {
            vault: Box::new(Unavailable),
            memory: Mutex::new(HashMap::new()),
        };
        let token = Secret::new("opaque-refresh");
        store.save("provider", &token).unwrap();
        assert!(store.session_only("provider"));
        assert_eq!(
            store.load("provider").unwrap().unwrap().expose(),
            token.expose()
        );
        store.delete("provider").unwrap();
        assert!(!store.session_only("provider"));
        assert!(store.load("provider").unwrap().is_none());
    }
}
