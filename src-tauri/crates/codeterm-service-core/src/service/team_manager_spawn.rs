//! A Team manager without a built-in name starts from its Team's Orchestrator only, with
//! `agent spawn --role <name>`: one live instance per name per Team, parented to the
//! Orchestrator and never task-bound. Its authority is the host's generic manager descriptor.

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

use codeterm_types::agent::role_kind::builtins;
use codeterm_types::team::staffing::{
    TEAM_ROLE_ALREADY_LIVE, TEAM_ROLE_NOT_TASK_BOUND, TEAM_ROLE_REQUIRES_ORCHESTRATOR,
};

use super::orchestrator::SpawnAgentParams;
use super::role_preset::is_custom_manager;
use super::team_roles::TeamRoleStore;
use super::ServiceContext;

/// What the admission decision reads, gathered without holding any lock across another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerSpawnFacts {
    pub name: String,
    /// The Team of the asking tab, when that tab is the Team's Orchestrator.
    pub orchestrator_team: Option<String>,
    pub registered: bool,
    pub live: bool,
    pub task_bound: bool,
}

/// The pure verdict: every refusal names its typed code first.
pub fn admit(facts: &ManagerSpawnFacts) -> Result<String, String> {
    let name = &facts.name;
    let team_id = facts.orchestrator_team.clone().ok_or_else(|| {
        format!(
            "{TEAM_ROLE_REQUIRES_ORCHESTRATOR}: manager `{name}` is started by its Team's Orchestrator with agent spawn --role {name}"
        )
    })?;
    if facts.task_bound {
        return Err(format!(
            "{TEAM_ROLE_NOT_TASK_BOUND}: manager `{name}` lives with the Team and takes no task binding"
        ));
    }
    if !facts.registered {
        return Err(format!(
            "{}: Team {team_id} has no manager `{name}`",
            codeterm_types::team::staffing::TEAM_MEMBER_ROLE_UNKNOWN
        ));
    }
    if facts.live {
        return Err(format!(
            "{TEAM_ROLE_ALREADY_LIVE}: Team {team_id} already runs manager `{name}`; send it work instead"
        ));
    }
    Ok(team_id)
}

/// Holds one `(team, name)` while its spawn runs, so two concurrent starts cannot both pass.
pub struct ManagerReservation {
    key: (String, String),
}

fn reserved() -> &'static Mutex<BTreeSet<(String, String)>> {
    static RESERVED: OnceLock<Mutex<BTreeSet<(String, String)>>> = OnceLock::new();
    RESERVED.get_or_init(|| Mutex::new(BTreeSet::new()))
}

impl Drop for ManagerReservation {
    fn drop(&mut self) {
        if let Ok(mut reserved) = reserved().lock() {
            reserved.remove(&self.key);
        }
    }
}

impl ServiceContext {
    /// Admits a custom manager spawn and parents it to the asking Orchestrator; any other role
    /// passes through untouched.
    pub fn admit_custom_manager_spawn(
        &self,
        params: &mut SpawnAgentParams,
    ) -> Result<Option<ManagerReservation>, String> {
        let name = match (params.role.as_deref(), params.role_profile.as_deref()) {
            (Some("Manager"), Some(name)) if is_custom_manager(name) => name.to_string(),
            _ => return Ok(None),
        };
        // Relaunching the manager's own tab in place (restart, restore) keeps its seat.
        let relaunch = params.existing_tab_id.as_deref().is_some_and(|tab_id| {
            self.mux.read().get_tab(tab_id).is_some_and(|tab| {
                tab.team_id.is_some()
                    && tab.role == Some(codeterm_types::agent::AgentRole::Manager)
                    && tab.role_kind_id() == Some(name.as_str())
            })
        });
        if relaunch {
            return Ok(None);
        }
        let caller = params
            .spawned_by_tab_id
            .clone()
            .or_else(|| params.parent_tab_id.clone());
        let orchestrator_team = caller.as_deref().and_then(|caller| {
            let mux = self.mux.read();
            mux.get_tab(caller)
                .filter(|tab| tab.role_kind_id() == Some(builtins::ORCHESTRATOR))
                .and_then(|tab| tab.team_id.clone())
        });
        let registered = orchestrator_team.as_deref().is_some_and(|team_id| {
            TeamRoleStore::global()
                .effective(Some(team_id))
                .iter()
                .any(|registration| registration.is_manager() && registration.name == name)
        });
        let live = orchestrator_team.as_deref().is_some_and(|team_id| {
            self.mux.read().all_tabs().any(|tab| {
                tab.team_id.as_deref() == Some(team_id)
                    && tab.role == Some(codeterm_types::agent::AgentRole::Manager)
                    && tab.role_kind_id() == Some(name.as_str())
            })
        });
        let facts = ManagerSpawnFacts {
            name: name.clone(),
            orchestrator_team,
            registered,
            live,
            task_bound: params.task_id.is_some() || params.plan_task.is_some(),
        };
        let team_id = admit(&facts)?;
        let key = (team_id.clone(), name.clone());
        {
            let mut reserved = reserved().lock().map_err(|_| "manager reservations poisoned")?;
            if !reserved.insert(key.clone()) {
                return Err(format!(
                    "{TEAM_ROLE_ALREADY_LIVE}: Team {team_id} is already starting manager `{name}`"
                ));
            }
        }
        params.parent_tab_id = caller;
        Ok(Some(ManagerReservation { key }))
    }
}

#[cfg(test)]
#[path = "team_manager_spawn/tests.rs"]
mod tests;
