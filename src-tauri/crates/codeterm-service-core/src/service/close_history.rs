use std::collections::HashSet;
use std::sync::LazyLock;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::Manager;

use super::{session_persist, ServiceContext};
use crate::multiplexer::state::StructureState;

static RESTORE_LOCK: Mutex<()> = Mutex::new(());
static CAPTURE_LOCK: Mutex<()> = Mutex::new(());
static WORKSPACE_CLOSES: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

#[cfg(test)]
mod tests;

fn restore_plan_metadata(current: &mut crate::plan::Plan, saved: &crate::plan::Plan) {
    current.team_id = saved.team_id.clone();
    current.owner = saved.owner.clone();
    current.planner = saved.planner.clone();
    current.status = saved.status.clone();
    current.orphaned = None;
}

pub(super) struct WorkspaceCloseGuard(String);

impl WorkspaceCloseGuard {
    pub fn new(id: &str) -> Self {
        WORKSPACE_CLOSES.lock().insert(id.into());
        Self(id.into())
    }
}

impl Drop for WorkspaceCloseGuard {
    fn drop(&mut self) {
        WORKSPACE_CLOSES.lock().remove(&self.0);
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Resources {
    plans: Vec<crate::plan::Plan>,
    crons: Vec<crate::cron::store::CronJob>,
    blueprints: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct SavedScope {
    id: String,
    workspace_id: String,
    team_id: Option<String>,
}

fn saved_blueprint(
    definition: &serde_json::Value,
) -> Result<crate::ports::sentinel::BlueprintRecord, String> {
    let scope = SavedScope::deserialize(definition).map_err(|error| error.to_string())?;
    Ok(crate::ports::sentinel::BlueprintRecord {
        id: scope.id,
        workspace_id: scope.workspace_id,
        team_id: scope.team_id,
        scenario_root: false,
        definition: definition.clone(),
    })
}

/// Restore is all-or-nothing, so every saved blueprint must parse before any effect runs.
fn validate_saved_blueprints(resources: &Resources) -> Result<(), String> {
    for saved in &resources.blueprints {
        saved_blueprint(saved)?;
        serde_json::from_value::<crate::sentinel::store::Sentinel>(saved.clone())
            .map_err(|error| format!("Saved blueprint is unreadable: {error}"))?;
    }
    Ok(())
}

impl ServiceContext {
    pub(crate) fn recover_close_inventories(&self) -> Result<(), String> {
        let records = self.mux.read().closed_structure_records()?;
        for record in records
            .into_iter()
            .filter(|record| record.state == StructureState::Captured)
        {
            self.settle_close_inventory(Some(&record.id))?;
        }
        Ok(())
    }

    pub(super) fn capture_close_inventory(
        &self,
        workspace: &str,
        team: Option<&str>,
    ) -> Result<Option<String>, String> {
        if team.is_some() && WORKSPACE_CLOSES.lock().contains(workspace) {
            return Ok(None);
        }
        let _capture = CAPTURE_LOCK.lock();
        let pending = self.mux.read().closed_structure_records()?;
        if pending.iter().any(|record| match team {
            Some(id) => record.teams.iter().any(|team| team.id == id),
            None => record.workspaces.iter().any(|saved| saved.id == workspace),
        }) {
            return Err("Close inventory is already being captured for this scope".into());
        }
        let id = format!("close_{}", uuid::Uuid::new_v4());
        let mut record = self.mux.read().capture_structure(
            workspace,
            team,
            id.clone(),
            super::team::now_ms(),
        )?;
        let team_ids: HashSet<_> = record.teams.iter().map(|team| team.id.as_str()).collect();
        let plans = self
            .plan_store
            .list_full()
            .into_iter()
            .filter(|plan| {
                plan.team_id
                    .as_deref()
                    .is_some_and(|id| team_ids.contains(id))
            })
            .collect();
        let in_scope = |workspace_id: &str, team_id: Option<&str>| {
            team.map_or_else(
                || workspace_id == workspace || team_id.is_some_and(|id| team_ids.contains(id)),
                |id| team_id == Some(id),
            )
        };
        let crons = self
            .app_handle
            .try_state::<crate::CronState>()
            .map(|state| {
                state
                    .lock()
                    .list()
                    .iter()
                    .filter(|job| in_scope(&job.workspace_id, job.team_id.as_deref()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let blueprints = self
            .app_handle
            .try_state::<crate::ports::sentinel::SentinelPorts>()
            .map(|ports| {
                ports
                    .store
                    .records()
                    .into_iter()
                    .filter(|job| in_scope(&job.workspace_id, job.team_id.as_deref()))
                    .map(|job| job.definition)
                    .collect()
            })
            .unwrap_or_default();
        record.resources = serde_json::to_value(Resources {
            plans,
            crons,
            blueprints,
        })
        .map_err(|error| error.to_string())?;
        self.mux.read().save_closed_structure(&record)?;
        Ok(Some(id))
    }

    pub(super) fn settle_close_inventory(&self, id: Option<&str>) -> Result<(), String> {
        let Some(id) = id else {
            return Ok(());
        };
        let mux = self.mux.read();
        let record = mux.closed_structure(id)?.ok_or("Close inventory missing")?;
        let mut changed = mux.get_workspace(&record.workspace.id).is_none()
            || record
                .tabs
                .iter()
                .any(|item| mux.get_tab(&item.tab.id).is_none())
            || record
                .teams
                .iter()
                .any(|team| mux.get_team(&team.id).is_none());
        drop(mux);
        let resources: Resources =
            serde_json::from_value(record.resources.clone()).map_err(|error| error.to_string())?;
        changed |= resources.plans.iter().any(|saved| {
            self.plan_store
                .get(&saved.id)
                .is_none_or(|current| current.team_id != saved.team_id)
        });
        if let Some(cron) = self.app_handle.try_state::<crate::CronState>() {
            changed |= resources
                .crons
                .iter()
                .any(|saved| cron.lock().get(&saved.id).is_none());
        }
        if let Some(blueprints) = self
            .app_handle
            .try_state::<crate::ports::sentinel::SentinelPorts>()
        {
            for saved in &resources.blueprints {
                changed |= blueprints
                    .store
                    .scope_of(&saved_blueprint(saved)?.id)
                    .is_none();
            }
        }
        let mux = self.mux.read();
        if changed {
            mux.finish_structure_close(id)?;
        } else {
            mux.discard_closed_structure(id)?;
        }
        drop(mux);
        self.emit_state_changed();
        Ok(())
    }

    pub(super) fn restore_close_inventory(
        &self,
        id: &str,
    ) -> Result<Option<(String, String)>, String> {
        let _restore = RESTORE_LOCK.lock();
        let Some(mut record) = self.mux.read().closed_structure(id)? else {
            return Ok(None);
        };
        let result = (
            record.workspace.id.clone(),
            record.workspace.active_tab_id.clone().unwrap_or_default(),
        );
        if record.state == StructureState::Restored {
            return Ok(Some(result));
        }
        if record.state == StructureState::Captured {
            return Err("Close has not completed".into());
        }
        let resources: Resources =
            serde_json::from_value(record.resources.clone()).map_err(|error| error.to_string())?;
        validate_saved_blueprints(&resources)?;
        for plan in &resources.plans {
            if self.plan_store.get(&plan.id).is_none() {
                self.ensure_plan_project_loaded(std::path::Path::new(&plan.project_path))
                    .map_err(|error| error.to_string())?;
            }
        }
        for item in &mut record.tabs {
            if item.tab.role == Some(crate::multiplexer::types::AgentRole::Worker) {
                if let Some(binding) = &item.tab.task_binding {
                    let origin = item.tab.cwd.clone();
                    let restore = item.tab.close_restore.get_or_insert_with(Default::default);
                    if restore.session_cwd.is_none() {
                        restore.session_cwd = origin;
                    }
                    let lane = binding
                        .plan_id
                        .as_deref()
                        .and_then(|id| self.plan_store.get(id))
                        .and_then(|plan| {
                            plan.tasks
                                .into_iter()
                                .find(|task| Some(task.id.as_str()) == binding.task_id.as_deref())
                        })
                        .and_then(|task| task.lane.worktree_path);
                    if let Some(path) = lane.filter(|path| std::path::Path::new(path).is_dir()) {
                        item.tab.cwd = Some(path);
                    } else if item
                        .tab
                        .cwd
                        .as_deref()
                        .is_some_and(|path| !std::path::Path::new(path).is_dir())
                    {
                        item.tab.cwd = record.workspace.cwd.clone();
                    }
                }
            }
        }
        record.restore_tabs()?;
        self.validate_restore_plans(&resources)?;
        record.state = StructureState::Restoring;
        self.mux.read().save_closed_structure(&record)?;
        self.mux.write().restore_structure(&record)?;
        self.persist_history_desk()?;
        self.restore_history_plans(&resources)?;
        self.restore_history_automation(&resources)?;
        record.state = StructureState::Restored;
        self.mux.read().save_closed_structure(&record)?;
        self.emit_state_changed();
        Ok(Some(result))
    }

    fn validate_restore_plans(&self, resources: &Resources) -> Result<(), String> {
        for saved in &resources.plans {
            let current = self
                .plan_store
                .get(&saved.id)
                .ok_or_else(|| format!("Plan {} is unavailable", saved.id))?;
            if current.team_id.is_some() && current.team_id != saved.team_id {
                return Err(format!("Plan {} belongs to another Team", saved.id));
            }
            if current.owner.tab_id.is_some() && current.owner.tab_id != saved.owner.tab_id {
                return Err(format!("Plan {} has another Orchestrator", saved.id));
            }
        }
        Ok(())
    }

    fn restore_history_plans(&self, resources: &Resources) -> Result<(), String> {
        for saved in &resources.plans {
            self.plan_store
                .with_plan(&saved.id, super::team::now_ms(), |current| {
                    restore_plan_metadata(current, saved);
                    Ok(())
                })
                .map_err(|error| error.to_string())?;
            self.emit_plan_changed(&saved.id);
        }
        Ok(())
    }

    fn restore_history_automation(&self, resources: &Resources) -> Result<(), String> {
        if !resources.crons.is_empty() {
            let cron = self
                .app_handle
                .try_state::<crate::CronState>()
                .ok_or("Cron store unavailable")?;
            for job in &resources.crons {
                crate::cron::store::commit(cron.inner(), |store| {
                    if let Some(existing) = store.get(&job.id) {
                        if existing.workspace_id != job.workspace_id
                            || existing.team_id != job.team_id
                        {
                            return Err(format!(
                                "Cron identity {} belongs to another scope",
                                job.id
                            ));
                        }
                        return Ok(crate::cron::store::Prepared::unchanged(()));
                    }
                    Ok(crate::cron::store::Prepared::new(
                        crate::cron::store::CronChange::Create(job.clone()),
                        (),
                    ))
                })
                .map_err(|error| error.to_string())?;
            }
            if let Some(wake) = self.app_handle.try_state::<crate::CronWake>() {
                wake.notify_all();
            }
            self.emit_cron_changed();
        }
        if !resources.blueprints.is_empty() {
            let ports = self
                .app_handle
                .try_state::<crate::ports::sentinel::SentinelPorts>()
                .ok_or("Blueprint store unavailable")?;
            for saved in &resources.blueprints {
                let job = saved_blueprint(saved)?;
                let existing = ports.store.scope_of(&job.id);
                if let Some((workspace_id, team_id)) = existing {
                    if workspace_id != job.workspace_id || team_id != job.team_id {
                        return Err(format!(
                            "Blueprint identity {} belongs to another scope",
                            job.id
                        ));
                    }
                    continue;
                }
                ports.store.insert_restored(&job)?;
            }
            ports.wake.notify();
        }
        Ok(())
    }

    pub fn rerun_restored_shell(&self, tab_id: &str) -> Result<(), String> {
        let _restore = RESTORE_LOCK.lock();
        let command = {
            let mut mux = self.mux.write();
            let tab = mux.get_tab_mut(tab_id).ok_or("Tab not found")?;
            if !tab.is_utility_shell() {
                return Err("Only restored utility shells can rerun".into());
            }
            let state = tab
                .close_restore
                .as_mut()
                .ok_or("Tab has no saved command")?;
            let command = state.claim_rerun()?;
            tab.utility_command_pending = command.is_some();
            command
        };
        self.persist_history_desk()?;
        self.ensure_tab_pty_for_send(tab_id)?;
        if let Some(command) = command {
            self.send_shell_cmd(tab_id, &command)?;
        }
        self.emit_state_changed();
        Ok(())
    }

    fn persist_history_desk(&self) -> Result<(), String> {
        match session_persist::write_session_snapshot(&self.app_handle, &self.mux)? {
            crate::session::store::SaveOutcome::Saved => Ok(()),
            crate::session::store::SaveOutcome::RefusedReduction(_) => {
                Err("Restored desk was not persisted".into())
            }
        }
    }
}
