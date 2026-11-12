//! Per-Team role registrations: durable, immutable once stored, and consulted by every
//! launch through the one role resolver. Registering never spawns or grants authority.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use codeterm_team_package::{is_role_name, ManagerSlot, RoleStart, TeamRoleGroup};
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

/// The label a tab shows for its registered role type: a display name the package declared,
/// or the name of a type the host has no builtin label for. Builtin types keep the host label.
pub fn role_label_for(
    team: &[RoleRegistration],
    role: codeterm_types::agent::AgentRole,
    kind_id: &str,
) -> Option<String> {
    // A kind that borrows another role's prose (the Watcher) never borrows its label.
    let registration = crate::service::role_preset::registration(team, role, kind_id)
        .filter(|registration| registration.name == kind_id)?;
    let declared = registration.label != registration.name;
    (declared || !builtins::is_builtin(kind_id)).then(|| registration.label.clone())
}

/// Stamps a launched tab's role label from its Team's registrations, read outside the mux lock.
pub fn stamp_role_label(svc: &crate::service::ServiceContext, tab_id: &str) {
    stamp_with(&svc.mux, tab_id, registrations);
}

fn stamp_with(
    mux: &crate::multiplexer::commands::MuxState,
    tab_id: &str,
    registrations: impl Fn(Option<&str>) -> Vec<RoleRegistration>,
) {
    let identity = mux.read().get_tab(tab_id).and_then(|tab| {
        Some((
            tab.team_id.clone(),
            tab.role?,
            tab.role_kind_id()?.to_string(),
        ))
    });
    let Some((team_id, role, kind_id)) = identity else {
        return;
    };
    let label = role_label_for(&registrations(team_id.as_deref()), role, &kind_id);
    if let Some(tab) = mux.write().get_tab_mut(tab_id) {
        // A placement that moved the tab while the registrations were read restamps itself.
        if tab.team_id == team_id {
            tab.role_label = label;
        }
    }
}

/// Restamps every member of a Team once its registrations land; the anchor got its Team
/// before any registration existed.
pub fn stamp_team_role_labels(svc: &crate::service::ServiceContext, team_id: &str) {
    stamp_team_with(&svc.mux, team_id, registrations);
    svc.emit_state_changed(crate::service::MuxEmitCause::AgentIdentity);
}

fn stamp_team_with(
    mux: &crate::multiplexer::commands::MuxState,
    team_id: &str,
    registrations: impl Fn(Option<&str>) -> Vec<RoleRegistration> + Copy,
) {
    let members = mux.read().team_member_tab_ids(team_id);
    for tab_id in members {
        stamp_with(mux, &tab_id, registrations);
    }
}

/// The Curator window a Team's curator registration schedules, clamped into the host bounds.
pub fn curator_window(team_id: Option<&str>) -> codeterm_types::plan::CuratorWindow {
    window_of(&registrations(team_id))
}

fn window_of(team: &[RoleRegistration]) -> codeterm_types::plan::CuratorWindow {
    team.iter()
        .find(|registration| registration.slot() == Some(ManagerSlot::Curator))
        .and_then(|registration| match &registration.start {
            RoleStart::Schedule { schedule } => codeterm_team_package::schedule_window(schedule),
            RoleStart::Mode(_) => None,
        })
        .map(|(min, max)| codeterm_types::plan::CuratorWindow::within_host(min, max))
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
