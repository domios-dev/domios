//! Per-Team role registrations: durable, immutable once stored, and consulted by every
//! launch through the one role resolver. Registering never spawns or grants authority.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use codeterm_team_package::{is_builtin_manager, is_role_name, RoleKind, RoleStart};
use codeterm_types::agent::role_kind::{builtins, RoleName, RoleType, RoleTypeError};
use codeterm_types::agent::AgentRole;
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
    if name == builtins::WATCHER || name == builtins::GENERAL_AGENT {
        return Err(format!("`{name}` is owned by the host and is never a package role"));
    }
    if registration.kind == RoleKind::Worker && is_builtin_manager(name) {
        return Err(format!("`{name}` is a built-in manager name, never a worker"));
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

    /// The roles the Team has: its registrations, or the default package as a whole when it
    /// has none (host policy, never a per-role fallback).
    pub fn effective(&self, team_id: Option<&str>) -> Vec<RoleRegistration> {
        let stored = team_id
            .filter(|team_id| valid_team_id(team_id).is_ok())
            .map(|team_id| self.registrations(team_id))
            .unwrap_or_default();
        if stored.is_empty() {
            crate::service::builtin_roles::default_registrations(team_id.unwrap_or_default())
        } else {
            stored
        }
    }

    /// Resolves `--role` against the Team: the registration's kind gives the structural role.
    /// Only the host-owned watcher resolves without one.
    pub fn role_type(&self, team_id: Option<&str>, value: &str) -> Result<RoleType, RoleTypeError> {
        resolve_role(&self.effective(team_id), value)
    }

    /// The worker `plan task dispatch` launches: `--role` must name a registered worker, and
    /// without it the Team's `worker`, only when the Team lists one.
    pub fn worker_type(
        &self,
        team_id: Option<&str>,
        requested: Option<&str>,
    ) -> Result<RoleType, RoleTypeError> {
        resolve_worker(&self.effective(team_id), requested)
    }
}

fn names_of(team: &[RoleRegistration], kind: Option<RoleKind>) -> Vec<String> {
    team.iter()
        .filter(|registration| kind.is_none_or(|kind| registration.kind == kind))
        .map(|registration| registration.name.clone())
        .collect()
}

/// A role name resolved against exactly the Team's roles.
pub fn resolve_role(team: &[RoleRegistration], value: &str) -> Result<RoleType, RoleTypeError> {
    let name = RoleName::parse(value)?;
    if let Some(host_owned) = name.host_owned() {
        return Ok(host_owned);
    }
    team.iter()
        .find(|registration| registration.name == name.as_str())
        .map(|registration| RoleType::new(AgentRole::from(registration.kind), &registration.name))
        .ok_or_else(|| RoleTypeError::NotRegistered {
            name: name.to_string(),
            roles: names_of(team, None),
        })
}

/// The dispatch rule: a registered worker by name, or `worker` by default when listed.
pub fn resolve_worker(
    team: &[RoleRegistration],
    requested: Option<&str>,
) -> Result<RoleType, RoleTypeError> {
    let workers = names_of(team, Some(RoleKind::Worker));
    let Some(value) = requested else {
        return if workers.iter().any(|name| name == builtins::WORKER) {
            Ok(RoleType::new(AgentRole::Worker, builtins::WORKER))
        } else {
            Err(RoleTypeError::NoDefaultWorker { workers })
        };
    };
    let name = RoleName::parse(value)?;
    match team
        .iter()
        .find(|registration| registration.name == name.as_str())
    {
        Some(registration) if registration.is_worker() => {
            Ok(RoleType::new(AgentRole::Worker, &registration.name))
        }
        Some(_) => Err(RoleTypeError::NotAWorker {
            name: name.to_string(),
            workers,
        }),
        None if name.is_manager_name() => Err(RoleTypeError::NotAWorker {
            name: name.to_string(),
            workers,
        }),
        None => Err(RoleTypeError::NotRegistered {
            name: name.to_string(),
            roles: workers,
        }),
    }
}

/// The registrations stored for a Team; empty means the Team is the default package as a whole,
/// which every resolver applies on its own.
pub fn registrations(team_id: Option<&str>) -> Vec<RoleRegistration> {
    team_id
        .filter(|team_id| valid_team_id(team_id).is_ok())
        .map(|team_id| TeamRoleStore::global().registrations(team_id))
        .unwrap_or_default()
}

/// The label a tab shows for its registered role: a display name the package declared, or the
/// name of a role the host has no builtin label for. Builtin names keep the host label.
pub fn role_label_for(
    team: &[RoleRegistration],
    role: codeterm_types::agent::AgentRole,
    kind_id: &str,
) -> Option<String> {
    let registration = crate::service::role_preset::registration(team, role, kind_id)?;
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

/// The Curator window a Team's curator manager schedules, clamped into the host bounds; `None`
/// when the Team has no curator manager with a schedule, so it runs no Curator rounds.
pub fn curator_window(team_id: Option<&str>) -> Option<codeterm_types::plan::CuratorWindow> {
    window_of(&TeamRoleStore::global().effective(team_id))
}

fn window_of(team: &[RoleRegistration]) -> Option<codeterm_types::plan::CuratorWindow> {
    team.iter()
        .find(|registration| registration.names(RoleKind::Manager, builtins::CURATOR))
        .and_then(|registration| match &registration.start {
            RoleStart::Schedule { schedule } => codeterm_team_package::schedule_window(schedule),
            RoleStart::Mode(_) => None,
        })
        .map(|(min, max)| codeterm_types::plan::CuratorWindow::within_host(min, max))
}

/// The Team's registration of this role, if the Team has it.
pub fn registration_for(
    team_id: Option<&str>,
    role: codeterm_types::agent::AgentRole,
    kind_id: &str,
) -> Option<RoleRegistration> {
    let team = TeamRoleStore::global().effective(team_id);
    crate::service::role_preset::registration(&team, role, kind_id).cloned()
}

#[cfg(test)]
#[path = "team_roles/tests.rs"]
mod tests;
