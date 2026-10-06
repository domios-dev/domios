use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::Manager;

use crate::multiplexer::state::MultiplexerState;
use crate::sentinel::{
    graph::{self, ConnectedCron, Identity, Snapshot},
    machine::{EscalateTarget, Route},
    scheduler::Handle,
    storage::{self, Staged},
    store::{Sentinel, Store},
};
use crate::service::ServiceContext;

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
    let job = store.get(id).ok_or("Sentinel not found")?;
    if job.workspace_id != workspace
        || job.team_id.as_deref() != team
        || !job.agent_authored
        || !graph::reachable_ids(store.list()).contains(id)
        || ((job.machine.stopped || job.expires_at.is_some_and(|at| store.clock.now_ms() >= at))
            && !store.has_live_lease(id))
    {
        return Err("Sentinel is unavailable in this automation scope".into());
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
                .ok_or("sentinels not initialized")?;
            let store = handle.store.lock();
            store.validate_target(job, id)?;
            exact = graph::identity(store.get(id).ok_or("Sentinel target disappeared")?);
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
                .try_state::<crate::CronState>()
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
    store: &crate::cron::store::CronStore,
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

impl ServiceContext {
    pub(crate) fn automation_graph_snapshot(
        &self,
        workspace: &str,
        team: Option<&str>,
    ) -> Result<Snapshot, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("sentinels not initialized")?;
        graph::validate_scope(&self.mux.read(), workspace, team)?;
        let (jobs, live_leases) = storage::committed_scope(&handle.store, workspace, team)?;
        let crons = self
            .app_handle
            .try_state::<crate::CronState>()
            .map(|state| connected_crons(&state.lock(), &jobs, workspace, team))
            .unwrap_or_default();
        let now = handle.store.lock().clock.now_ms();
        let executor = self.automation_decision_executor();
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
        graph::project_worker_states(&mut snapshot, &mux, |tab| self.resolved_tab_status(tab));
        Ok(snapshot)
    }

    pub(crate) fn automation_graph_workspace_snapshot(
        &self,
        workspace: &str,
    ) -> Result<Snapshot, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("sentinels not initialized")?;
        {
            let mux = self.mux.read();
            graph::validate_scope(&mux, workspace, None)?;
        }
        let (jobs, live_leases) = storage::committed_workspace(&handle.store, workspace)?;
        let mut grouped: BTreeMap<Option<String>, Vec<Sentinel>> = BTreeMap::new();
        for job in jobs
            .iter()
            .filter(|job| job.agent_authored && job.scenario_id.as_deref() == Some(job.id.as_str()))
        {
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
        let executor = self.automation_decision_executor();
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
                .try_state::<crate::CronState>()
                .map(|state| connected_crons(&state.lock(), &scoped_jobs, workspace, team_ref))
                .unwrap_or_default();
            let mut snapshot = graph::project(
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
        graph::project_worker_states(&mut combined, &mux, |tab| self.resolved_tab_status(tab));
        Ok(combined)
    }

    fn automation_decision_executor(&self) -> Value {
        let config = self
            .app_handle
            .try_state::<crate::config::ConfigState>()
            .map(|s| s.lock().supporter.clone());
        let adapter = config
            .as_ref()
            .and_then(|c| c.active_id())
            .and_then(|id| self.supporter_models.get(id));
        let configured = config
            .as_ref()
            .and_then(|c| c.active_id().and_then(|id| c.connection(id).model));
        let capabilities = adapter.as_ref().map(|a| {
            let p = a.primitives();
            let mut names = Vec::new();
            if p.choice {
                names.push("choice");
            }
            if p.noul {
                names.push("noul");
            }
            if p.score {
                names.push("score");
            }
            names
        });
        json!({"kind":"decision","selection":"shared_active","provider_id":null,"adapter_id":config.as_ref().and_then(|c| c.active_id()),"model":configured,"configured_model":configured,"reported_model":null,"capabilities":capabilities})
    }

    pub(crate) fn automation_graph_stop_host(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
    ) -> Result<Sentinel, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("sentinels not initialized")?;
        let staged = {
            let mux = self.mux.read();
            host_stop_staged(&mut handle.store.lock(), &mux, workspace, team, id)
        };
        let result = staged.and_then(|staged| staged.settle(&handle.store));
        handle.notify();
        result
    }

    pub(crate) fn automation_graph_delete_host(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
    ) -> Result<(), String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("sentinels not initialized")?;
        let staged = {
            let mux = self.mux.read();
            graph::host_delete_staged(&mut handle.store.lock(), &mux, workspace, team, id)?
        };
        let result = staged.settle(&handle.store);
        handle.notify();
        result
    }

    pub(crate) fn automation_graph_stop_tab(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
        caller_tab: &str,
        nonce: Option<&str>,
    ) -> Result<Sentinel, String> {
        let caller = self.sentinel_caller(caller_tab, nonce)?;
        if caller.workspace != workspace || caller.team.as_deref() != team {
            return Err("Sentinel caller scope differs from graph scope".into());
        }
        graph::validate_scope(&self.mux.read(), workspace, team)?;
        self.sentinel_stop(&caller, id)
    }
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
                    .filter(|j| j.agent_authored && j.scenario_id.as_deref() == Some(j.id.as_str()))
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
                crate::api::events::emit_everywhere(&app, "automation-graph-changed", change);
            }
        });
    }
}
