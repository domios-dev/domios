//! Per-Team role registrations: durable, immutable once stored, and consulted by every
//! launch through the one role resolver. Registering never spawns or grants authority.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use codeterm_team_package::{is_role_name, ManagerSlot, TeamRoleGroup};
use codeterm_types::agent::role_kind::{builtins, RoleType, RoleTypeError};
use codeterm_types::team::role_registration::{RoleRegistration, RoleRegistrationOutcome};
use parking_lot::Mutex;

use crate::persisted_state::{Loaded, StateFile};

const FILE_NAME: &str = "roles.json";
const MAX_REGISTRATIONS: usize = 64;

/// One `roles.json` per Team under `<root>/<team_id>/`, written atomically with backups.
pub struct TeamRoleStore {
    root: PathBuf,
    cache: Mutex<HashMap<String, Vec<RoleRegistration>>>,
}

fn valid_team_id(team_id: &str) -> Result<(), String> {
    let valid = !team_id.is_empty()
        && team_id.len() <= 128
        && team_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(format!("`{team_id}` is not a Team id"))
    }
}

fn validate(registration: &RoleRegistration) -> Result<(), String> {
    valid_team_id(&registration.team_id)?;
    let name = registration.name.as_str();
    if !is_role_name(name) {
        return Err(format!("`{name}` is not a role name"));
    }
    match registration.group {
        TeamRoleGroup::Manager if ManagerSlot::parse(name).is_none() => {
            return Err(format!("`{name}` is not a manager slot"));
        }
        TeamRoleGroup::Worker
            if ManagerSlot::parse(name).is_some()
                || name == builtins::WATCHER
                || name == builtins::GENERAL_AGENT =>
        {
            return Err(format!("`{name}` is reserved for a host or manager role"));
        }
        _ => {}
    }
    if registration.instructions.trim().is_empty() {
        return Err(format!("`{name}` carries no instructions"));
    }
    if registration.label.trim().is_empty() {
        return Err(format!("`{name}` carries no label"));
    }
    Ok(())
}

impl TeamRoleStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The instance store under the data directory.
    pub fn global() -> &'static Self {
        static STORE: OnceLock<TeamRoleStore> = OnceLock::new();
        STORE.get_or_init(|| Self::new(crate::paths::data_dir().join("teams")))
    }

    pub fn path(&self, team_id: &str) -> PathBuf {
        self.root.join(team_id).join(FILE_NAME)
    }

    fn file(&self, team_id: &str) -> Result<StateFile, String> {
        valid_team_id(team_id)?;
        Ok(StateFile::new(self.path(team_id)))
    }

    fn load(&self, team_id: &str) -> Vec<RoleRegistration> {
        let Ok(file) = self.file(team_id) else {
            return Vec::new();
        };
        match file.load(|text| {
            serde_json::from_str::<Vec<RoleRegistration>>(text).map_err(|error| error.to_string())
        }) {
            Loaded::Parsed(registrations)
            | Loaded::Recovered {
                value: registrations,
                ..
            } => registrations,
            Loaded::Missing | Loaded::Unrecoverable { .. } => Vec::new(),
        }
    }

    /// The Team's registrations; a Team with none resolves the builtin default package.
    pub fn registrations(&self, team_id: &str) -> Vec<RoleRegistration> {
        if let Some(cached) = self.cache.lock().get(team_id) {
            return cached.clone();
        }
        let loaded = self.load(team_id);
        self.cache
            .lock()
            .entry(team_id.to_string())
            .or_insert(loaded)
            .clone()
    }

    /// Stores a new role; an existing one stays pinned to what it was registered as.
    pub fn register(
        &self,
        registration: RoleRegistration,
    ) -> Result<RoleRegistrationOutcome, String> {
        validate(&registration)?;
        let team_id = registration.team_id.clone();
        let file = self.file(&team_id)?;
        let mut cache = self.cache.lock();
        let mut current = match cache.get(&team_id) {
            Some(cached) => cached.clone(),
            None => self.load(&team_id),
        };
        if let Some(existing) = current
            .iter()
            .find(|existing| existing.name == registration.name)
        {
            return Ok(if existing.same_content(&registration) {
                RoleRegistrationOutcome::AlreadyPresent
            } else {
                RoleRegistrationOutcome::Pinned
            });
        }
        if current.len() >= MAX_REGISTRATIONS {
            return Err(format!(
                "Team `{team_id}` already registers {MAX_REGISTRATIONS} roles"
            ));
        }
        current.push(registration);
        let text = serde_json::to_string_pretty(&current).map_err(|error| error.to_string())?;
        file.save(&text)?;
        cache.insert(team_id, current);
        Ok(RoleRegistrationOutcome::Created)
    }

    /// Removes the registrations of a Team that never came up (a rolled-back pair mint).
    pub fn forget(&self, team_id: &str) -> Result<(), String> {
        let file = self.file(team_id)?;
        let mut cache = self.cache.lock();
        for path in std::iter::once(file.path().to_path_buf()).chain(file.backups()) {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("remove {}: {error}", path.display())),
            }
        }
        // Only empty folders go; anything else under the Team folder is not ours.
        let _ = std::fs::remove_dir(file.backup_dir());
        let _ = std::fs::remove_dir(self.root.join(team_id));
        cache.remove(team_id);
        Ok(())
    }

    /// Drops cached registrations so the next read comes from disk.
    pub fn reload(&self, team_id: Option<&str>) {
        let mut cache = self.cache.lock();
        match team_id {
            Some(team_id) => {
                cache.remove(team_id);
            }
            None => cache.clear(),
        }
    }

    /// Parses `--role` and checks that a worker type is available to the Team.
    pub fn role_type(&self, team_id: Option<&str>, value: &str) -> Result<RoleType, RoleTypeError> {
        let role_type = RoleType::parse(value)?;
        if !role_type.is_worker() {
            return Ok(role_type);
        }
        let builtin = crate::service::builtin_roles::builtin_workers()
            .iter()
            .any(|(name, _)| *name == role_type.kind_id);
        let registered = team_id.is_some_and(|team_id| {
            self.registrations(team_id).iter().any(|registration| {
                registration.group == TeamRoleGroup::Worker
                    && registration.name == role_type.kind_id
            })
        });
        if builtin || registered {
            Ok(role_type)
        } else {
            Err(RoleTypeError::UnavailableWorker(role_type.kind_id))
        }
    }
}

/// The registrations every launch composes against.
pub fn registrations(team_id: Option<&str>) -> Vec<RoleRegistration> {
    team_id
        .filter(|team_id| valid_team_id(team_id).is_ok())
        .map(|team_id| TeamRoleStore::global().registrations(team_id))
        .unwrap_or_default()
}

/// The registration that names this role type for the Team, if any.
pub fn registration_for(
    team_id: Option<&str>,
    role: codeterm_types::agent::AgentRole,
    kind_id: &str,
) -> Option<RoleRegistration> {
    let team = registrations(team_id);
    crate::service::role_preset::registration(&team, role, kind_id).cloned()
}

#[cfg(test)]
#[path = "team_roles/tests.rs"]
mod tests;
