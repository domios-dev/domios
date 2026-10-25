use crate::{Error, Result, Secret};
use codeterm_util::secret_file::{SecretFile, SecretFileError};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

pub struct ProtectedCredentials {
    file: SecretFile,
    state: Mutex<State>,
}
#[derive(Default)]
struct State {
    memory: HashMap<String, Secret>,
    migrating: HashSet<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum MigrationDecision {
    Skip,
    Claim,
}
fn migration_decision(present: bool, completed: bool) -> MigrationDecision {
    if present || completed {
        MigrationDecision::Skip
    } else {
        MigrationDecision::Claim
    }
}

#[derive(Debug, PartialEq, Eq)]
enum MigrationCompletion {
    Persist,
    MarkAbsent,
    Retry,
}

fn migration_completion(read: &Result<Option<Secret>>) -> MigrationCompletion {
    match read {
        Ok(Some(_)) => MigrationCompletion::Persist,
        Ok(None) => MigrationCompletion::MarkAbsent,
        Err(_) => MigrationCompletion::Retry,
    }
}

impl ProtectedCredentials {
    pub fn new(data_root: impl AsRef<Path>) -> Self {
        Self {
            file: SecretFile::new_at(data_root, "domios.account"),
            state: Mutex::new(State::default()),
        }
    }
    pub fn session_only(&self, name: &str) -> bool {
        self.state.lock().memory.contains_key(name)
    }
    pub fn load(&self, name: &str) -> Result<Option<Secret>> {
        let state = self.state.lock();
        if let Some(value) = state.memory.get(name) {
            return Ok(Some(value.clone()));
        }
        match self.file.get(name) {
            Ok(value) => Ok(Some(Secret::new(value))),
            Err(SecretFileError::NotFound) => Ok(None),
            Err(_) => Err(Error::Credentials),
        }
    }
    pub fn save(&self, name: &str, token: &Secret) -> Result<()> {
        let mut state = self.state.lock();
        state.migrating.remove(name);
        self.persist(&mut state, name, token)
    }
    fn persist(&self, state: &mut State, name: &str, token: &Secret) -> Result<()> {
        if self.file.is_available() && self.file.set(name, token.expose()).is_ok() {
            state.memory.remove(name);
        } else {
            log::warn!("[account] Private credential file unavailable; sign-in session is kept in memory only and will not survive restart");
            state.memory.insert(name.to_owned(), token.clone());
        }
        Ok(())
    }
    pub fn delete(&self, name: &str) -> Result<()> {
        let mut state = self.state.lock();
        state.migrating.remove(name);
        state.memory.remove(name);
        self.file
            .set(&migration_key(name), "attempted")
            .map_err(|_| Error::Credentials)?;
        self.file.delete(name).map_err(|_| Error::Credentials)
    }
    pub fn claim_migration(&self, name: &str) -> Result<bool> {
        let mut state = self.state.lock();
        let present = state.memory.contains_key(name)
            || self.file.has(name).map_err(|_| Error::Credentials)?;
        let attempted = self
            .file
            .has(&migration_key(name))
            .map_err(|_| Error::Credentials)?;
        if migration_decision(present, attempted || state.migrating.contains(name))
            == MigrationDecision::Skip
        {
            return Ok(false);
        }
        if !self.file.is_available() {
            return Ok(false);
        }
        state.migrating.insert(name.to_owned());
        Ok(true)
    }
    pub fn complete_migration(&self, name: &str, read: Result<Option<Secret>>) -> Result<bool> {
        match migration_completion(&read) {
            MigrationCompletion::Persist => {
                if let Ok(Some(token)) = read {
                    return self.finish_migration(name, &token);
                }
                Ok(false)
            }
            MigrationCompletion::MarkAbsent => {
                let mut state = self.state.lock();
                if state.migrating.remove(name) {
                    self.file
                        .set(&migration_key(name), "attempted")
                        .map_err(|_| Error::Credentials)?;
                }
                Ok(false)
            }
            MigrationCompletion::Retry => {
                self.state.lock().migrating.remove(name);
                Ok(false)
            }
        }
    }
    pub fn finish_migration(&self, name: &str, token: &Secret) -> Result<bool> {
        let mut state = self.state.lock();
        if !state.migrating.remove(name) {
            return Ok(false);
        }
        self.persist(&mut state, name, token)?;
        Ok(!state.memory.contains_key(name))
    }
}
fn migration_key(name: &str) -> String {
    format!("migration:{name}")
}

#[cfg(test)]
mod tests;
