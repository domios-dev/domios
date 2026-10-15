mod files;
pub use files::AutomationFilesExt;
mod pipeline;
use pipeline::project_pipeline_contract;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::Manager;

use super::SentinelExt;
use crate::{
    graph::{self, ConnectedCron, Identity, Snapshot},
    machine::{EscalateTarget, Route},
    scheduler::Handle,
    storage::{self, Staged},
    store::{Sentinel, Store},
};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::ServiceContext;
use codeterm_service_core::storage::BlueprintJournalEntry;

pub fn explicit_team(value: &Value) -> Result<Option<String>, String> {
    match value {
        Value::Null => Ok(None),
        Value::String(team) if !team.is_empty() => Ok(Some(team.clone())),
        _ => Err("team_id must be an explicit null or nonempty Team ID".into()),
    }
}

#[cfg(test)]
pub fn host_stop(
    store: &mut Store,
    mux: &MultiplexerState,
    workspace: &str,
    team: Option<&str>,
    id: &str,
) -> Result<Sentinel, String> {
    host_stop_gate(store, mux, workspace, team, id)?;
    store.stop(id, "stopped_by_host")
}

/// Authorizes the host stop under the mux and store locks; settle it after both drop.
pub fn host_stop_staged(
    store: &mut Store,
    mux: &MultiplexerState,
    workspace: &str,
    team: Option<&str>,
    id: &str,
) -> Result<Staged<Sentinel>, String> {
    host_stop_gate(store, mux, workspace, team, id)?;
    Ok(store.stage_stop(id, "stopped_by_host"))
}

fn host_stop_gate(
    store: &Store,
    mux: &MultiplexerState,
    workspace: &str,
    team: Option<&str>,
    id: &str,
) -> Result<(), String> {
    graph::validate_scope(mux, workspace, team)?;
    store.ensure_available()?;
    let job = store.get(id).ok_or("Blueprint not found")?;
    if job.workspace_id != workspace
        || job.team_id.as_deref() != team
        || !graph::reachable_ids(store.list()).contains(id)
        || ((job.machine.stopped || job.expires_at.is_some_and(|at| store.clock.now_ms() >= at))
            && !store.has_live_lease(id))
    {
        return Err("Blueprint is unavailable in this automation scope".into());
    }
    Ok(())
}

pub fn target_identity(
    svc: &ServiceContext,
    job: &Sentinel,
    route: &Route,
) -> Result<Option<Identity>, String> {
    let mut exact = Identity {
        kind: String::new(),
        id: String::new(),
        workspace_id: job.workspace_id.clone(),
        team_id: job.team_id.clone(),
        owner_tab_id: None,
    };
    match route {
        Route::Sentinel(id) => {
            let handle = svc
                .app_handle
                .try_state::<Handle>()
                .ok_or("blueprints not initialized")?;
            let store = handle.store.lock();
            store.validate_target(job, id)?;
            exact = graph::identity(store.get(id).ok_or("Blueprint target disappeared")?);
        }
        Route::Send(id) => {
            let mux = svc.mux.read();
            let tab = mux.get_tab(id).ok_or("send target disappeared")?;
            if tab.team_id != job.team_id
                || mux.find_workspace_for_tab(id).as_deref() != Some(&job.workspace_id)
            {
                return Err("send target scope changed".into());
            }
            exact = graph::tab_identity(tab, &job.workspace_id);
        }
        Route::Cron(id) => {
            let crons = svc
                .app_handle
                .try_state::<codeterm_service_core::CronState>()
                .ok_or("cron not initialized")?;
            let store = crons.lock();
            let cron = store.get(id).ok_or("cron target disappeared")?;
            if cron.team_id != job.team_id || cron.workspace_id != job.workspace_id {
                return Err("cron target scope changed".into());
            }
            exact.kind = "cron".into();
            exact.id = id.clone();
            exact.owner_tab_id = cron.creator_tab_id.clone();
        }
        Route::Escalate(EscalateTarget::Agent(id)) => {
            let mux = svc.mux.read();
            let tab = mux.get_tab(id).ok_or("escalation target disappeared")?;
            if tab.agent_type.is_none()
                || tab.team_id != job.team_id
                || mux.find_workspace_for_tab(id).as_deref() != Some(&job.workspace_id)
            {
                return Err("escalation target scope changed".into());
            }
            exact = graph::tab_identity(tab, &job.workspace_id);
        }
        Route::Escalate(EscalateTarget::Creator) => return Ok(None),
        _ => return Ok(None),
    }
    Ok(Some(exact))
}

pub fn connected_crons(
    store: &codeterm_service_core::cron::store::CronStore,
    jobs: &[Sentinel],
    workspace: &str,
    team: Option<&str>,
) -> BTreeMap<String, ConnectedCron> {
    let reachable = graph::reachable_ids(jobs);
    let targets: BTreeMap<_, _> = jobs
        .iter()
        .filter(|job| reachable.contains(&job.id))
        .flat_map(|job| job.routes.values().flatten())
        .filter_map(|handler| {
            if let Route::Cron(id) = &handler.action {
                Some((id.clone(), handler.target.clone()))
            } else {
                None
            }
        })
        .collect();
    let mut crons = BTreeMap::new();
    for (id, recorded) in targets {
        let current = store.get(&id);
        if current
            .is_some_and(|cron| cron.workspace_id != workspace || cron.team_id.as_deref() != team)
        {
            continue;
        }
        let job = current.cloned();
        let identity = job
            .as_ref()
            .map(|cron| Identity {
                kind: "cron".into(),
                id: id.clone(),
                workspace_id: workspace.into(),
                team_id: team.map(str::to_owned),
                owner_tab_id: cron.creator_tab_id.clone(),
            })
            .or(recorded);
        let Some(identity) = identity else { continue };
        if identity.kind != "cron"
            || identity.id != id
            || identity.workspace_id != workspace
            || identity.team_id.as_deref() != team
        {
            continue;
        }
        let mut runs: Vec<_> = store
            .get_history(&id)
            .into_iter()
            .rev()
            .take(graph::MAX_RUNS_PER_CRON + 1)
            .cloned()
            .collect();
        if let Some(last) = job.as_ref().and_then(|cron| cron.last_run.as_ref()) {
            if !runs.iter().any(|run| run.run_id == last.run_id) {
                runs.insert(0, last.clone());
            }
        }
        crons.insert(
            id,
            ConnectedCron {
                job,
                identity,
                runs,
            },
        );
    }
    crons
}

pub trait AutomationGraphExt {
    fn automation_graph_snapshot(
        &self,
        workspace: &str,
        team: Option<&str>,
    ) -> Result<Snapshot, String>;
    fn automation_graph_workspace_snapshot(&self, workspace: &str) -> Result<Snapshot, String>;
    fn automation_configured_fallback(&self) -> Value;
    fn automation_graph_stop_host(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
    ) -> Result<Sentinel, String>;
    fn automation_graph_delete_host(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
    ) -> Result<(), String>;
    fn automation_graph_stop_tab(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
        caller_tab: &str,
        nonce: Option<&str>,
    ) -> Result<Sentinel, String>;
}

impl AutomationGraphExt for ServiceContext {
    fn automation_graph_snapshot(
        &self,
        workspace: &str,
        team: Option<&str>,
    ) -> Result<Snapshot, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        graph::validate_scope(&self.mux.read(), workspace, team)?;
        let (jobs, live_leases) = storage::committed_scope(&handle.store, workspace, team)?;
        let journals = pipeline_journals(&handle, &jobs)?;
        let crons = self
            .app_handle
            .try_state::<codeterm_service_core::CronState>()
            .map(|state| connected_crons(&state.lock(), &jobs, workspace, team))
            .unwrap_or_default();
        let now = handle.store.lock().clock.now_ms();
        let executor = self.automation_configured_fallback();
        let mux = self.mux.read();
        let mut snapshot = graph::project(
            workspace,
            team,
            &jobs,
            &mux,
            &crons,
            executor,
            now,
            &live_leases,
        )?;
        project_pipeline_contract(
            &mut snapshot,
            &jobs,
            &journals,
            &live_leases,
            self.ext::<codeterm_plugin::graph_events::SubscriptionTable>(),
        );
        graph::project_worker_states(&mut snapshot, &mux, |tab| self.resolved_tab_status(tab));
        Ok(snapshot)
    }

    fn automation_graph_workspace_snapshot(&self, workspace: &str) -> Result<Snapshot, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        {
            let mux = self.mux.read();
            graph::validate_scope(&mux, workspace, None)?;
        }
        let (jobs, live_leases) = storage::committed_workspace(&handle.store, workspace)?;
        let journals = pipeline_journals(&handle, &jobs)?;
        let mut grouped: BTreeMap<Option<String>, Vec<Sentinel>> = BTreeMap::new();
        for job in jobs.iter().filter(|job| job.is_scenario_root()) {
            grouped.entry(job.team_id.clone()).or_default();
        }
        if grouped.is_empty() {
            grouped.insert(None, Vec::new());
        }
        for job in jobs.iter().filter(|job| job.workspace_id == workspace) {
            if let Some(scope) = grouped.get_mut(&job.team_id) {
                scope.push(job.clone());
            }
        }
        let now = handle.store.lock().clock.now_ms();
        let executor = self.automation_configured_fallback();
        let mux = self.mux.read();
        let mut combined = graph::project(
            workspace,
            None,
            &[],
            &mux,
            &BTreeMap::new(),
            executor.clone(),
            now,
            &live_leases,
        )?;
        let mut revisions = Vec::new();
        for (team, scoped_jobs) in grouped {
            let team_ref = team.as_deref();
            let crons = self
                .app_handle
                .try_state::<codeterm_service_core::CronState>()
                .map(|state| connected_crons(&state.lock(), &scoped_jobs, workspace, team_ref))
                .unwrap_or_default();
            let snapshot = graph::project(
                workspace,
                team_ref,
                &scoped_jobs,
                &mux,
                &crons,
                executor.clone(),
                now,
                &live_leases,
            )?;
            revisions.push(format!(
                "{}:{}",
                team.as_deref().unwrap_or_default(),
                snapshot.revision
            ));
            combined.runs.extend(snapshot.runs);
            combined.truncated |= snapshot.truncated;
            let scenario_room = graph::MAX_NODES.saturating_sub(combined.scenarios.len());
            if snapshot.scenarios.len() > scenario_room {
                combined.truncated = true;
            }
            combined
                .scenarios
                .extend(snapshot.scenarios.into_iter().take(scenario_room));
            let node_room = graph::MAX_NODES.saturating_sub(combined.nodes.len());
            if snapshot.nodes.len() > node_room {
                combined.truncated = true;
            }
            combined
                .nodes
                .extend(snapshot.nodes.into_iter().take(node_room));
            let edge_room = graph::MAX_EDGES.saturating_sub(combined.edges.len());
            if snapshot.edges.len() > edge_room {
                combined.truncated = true;
            }
            combined
                .edges
                .extend(snapshot.edges.into_iter().take(edge_room));
        }
        combined.revision = format!("{:x}", Sha256::digest(revisions.join("|")));
        combined.all_teams = true;
        for node in &mut combined.nodes {
            if let Some(detail) = node.get_mut("detail") {
                detail["definition"] = json!({"unavailable":"team_view_required"});
            }
        }
        project_pipeline_contract(
            &mut combined,
            &jobs,
            &journals,
            &live_leases,
            self.ext::<codeterm_plugin::graph_events::SubscriptionTable>(),
        );
        graph::project_worker_states(&mut combined, &mux, |tab| self.resolved_tab_status(tab));
        Ok(combined)
    }

    fn automation_configured_fallback(&self) -> Value {
        let config = self
            .app_handle
            .try_state::<codeterm_service_core::config::ConfigState>()
            .map(|s| s.lock().supporter.clone());
        match config
            .as_ref()
            .and_then(|config| config.active_id().map(|id| (config, id)))
        {
            Some((config, id)) => {
                graph::configured_fallback(Some(id), config.connection(id).model.as_deref())
            }
            None => Value::Null,
        }
    }

    fn automation_graph_stop_host(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
    ) -> Result<Sentinel, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let staged = {
            let mux = self.mux.read();
            host_stop_staged(&mut handle.store.lock(), &mux, workspace, team, id)
        };
        let result = staged.and_then(|staged| staged.settle(&handle.store));
        handle.notify();
        result
    }

    fn automation_graph_delete_host(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
    ) -> Result<(), String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let staged = {
            let mux = self.mux.read();
            graph::host_delete_staged(&mut handle.store.lock(), &mux, workspace, team, id)?
        };
        let result = staged.settle(&handle.store);
        handle.notify();
        result
    }

    fn automation_graph_stop_tab(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
        caller_tab: &str,
        nonce: Option<&str>,
    ) -> Result<Sentinel, String> {
        let caller = self.sentinel_caller(caller_tab, nonce)?;
        if caller.workspace != workspace || caller.team.as_deref() != team {
            return Err("Blueprint caller scope differs from graph scope".into());
        }
        graph::validate_scope(&self.mux.read(), workspace, team)?;
        self.sentinel_stop(&caller, id)
    }
}

fn pipeline_journals(
    handle: &Handle,
    jobs: &[Sentinel],
) -> Result<BTreeMap<String, Vec<BlueprintJournalEntry>>, String> {
    let ids = jobs
        .iter()
        .filter(|job| job.pipeline().is_some())
        .map(|job| job.id.clone())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let completion = {
        let store = handle.store.lock();
        store.typed_pipeline_logs(ids, 200)?
    };
    completion
        .wait(crate::storage::COMMIT_DEADLINE)
        .map(|read| read.value)
        .map_err(|error| error.to_string())
}

pub fn install_invalidations(
    svc: &ServiceContext,
    store: std::sync::Arc<parking_lot::Mutex<Store>>,
) {
    use tauri::Listener;
    for event in [
        "cron-fired",
        "cron-run-finished",
        "mux-state-changed",
        "tab-status-changed",
        "config-changed",
    ] {
        let store = store.clone();
        let app = svc.app_handle.clone();
        svc.app_handle.listen_any(event, move |_| {
            let changes = {
                let store = store.lock();
                let scopes: BTreeSet<_> = store
                    .list()
                    .iter()
                    .filter(|j| {
                        j.is_scenario_root()
                    })
                    .map(|j| (j.workspace_id.clone(), j.team_id.clone()))
                    .collect();
                scopes
                    .into_iter()
                    .filter_map(|(workspace_id, team_id)| {
                        let revision = store.committed_revision(&workspace_id, team_id.as_deref())?;
                        Some(json!({"workspace_id":workspace_id,"team_id":team_id,"revision":revision}))
                    })
                    .collect::<Vec<_>>()
            };
            for change in changes {
                codeterm_host_runtime::events::emit_everywhere(&app, "automation-graph-changed", change);
            }
        });
    }
}
