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
use crate::storage::BlueprintJournalEntry;

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
        || (!job.agent_authored && job.pipeline.is_none())
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
            .ok_or("blueprints not initialized")?;
        graph::validate_scope(&self.mux.read(), workspace, team)?;
        let (jobs, live_leases) = storage::committed_scope(&handle.store, workspace, team)?;
        let journals = pipeline_journals(&handle, &jobs)?;
        let crons = self
            .app_handle
            .try_state::<crate::CronState>()
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
            &self.graph_event_subscriptions,
        );
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
            .ok_or("blueprints not initialized")?;
        {
            let mux = self.mux.read();
            graph::validate_scope(&mux, workspace, None)?;
        }
        let (jobs, live_leases) = storage::committed_workspace(&handle.store, workspace)?;
        let journals = pipeline_journals(&handle, &jobs)?;
        let mut grouped: BTreeMap<Option<String>, Vec<Sentinel>> = BTreeMap::new();
        for job in jobs.iter().filter(|job| {
            (job.agent_authored || job.pipeline.is_some())
                && job.scenario_id.as_deref() == Some(job.id.as_str())
        }) {
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
                .try_state::<crate::CronState>()
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
            &self.graph_event_subscriptions,
        );
        graph::project_worker_states(&mut combined, &mux, |tab| self.resolved_tab_status(tab));
        Ok(combined)
    }

    fn automation_configured_fallback(&self) -> Value {
        let config = self
            .app_handle
            .try_state::<crate::config::ConfigState>()
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

    pub(crate) fn automation_graph_stop_host(
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

    pub(crate) fn automation_graph_delete_host(
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
        .filter(|job| job.pipeline.is_some())
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
        .wait(crate::sentinel::storage::COMMIT_DEADLINE)
        .map(|read| read.value)
        .map_err(|error| error.to_string())
}

fn project_pipeline_contract(
    snapshot: &mut Snapshot,
    jobs: &[Sentinel],
    journals: &BTreeMap<String, Vec<BlueprintJournalEntry>>,
    live_leases: &BTreeSet<String>,
    event_subscriptions: &codeterm_plugin::graph_events::SubscriptionTable,
) {
    let mut journal_revisions = jobs
        .iter()
        .filter(|job| job.pipeline.is_some())
        .filter_map(|job| {
            journals
                .get(&job.id)
                .and_then(|entries| entries.first())
                .map(|entry| (job.id.as_str(), entry.entry_id))
        })
        .collect::<Vec<_>>();
    let mut event_revisions = jobs
        .iter()
        .filter(|job| {
            job.pipeline.as_ref().is_some_and(|spec| {
                matches!(
                    &spec.trigger,
                    codeterm_types::blueprint_pipeline::Trigger::Event { .. }
                )
            })
        })
        .filter_map(|job| {
            event_subscriptions
                .summary(&job.id)
                .map(|summary| (job.id.as_str(), summary))
        })
        .collect::<Vec<_>>();
    if !journal_revisions.is_empty() || !event_revisions.is_empty() {
        journal_revisions.sort_unstable();
        event_revisions.sort_by_key(|(id, _)| *id);
        let mut digest = Sha256::new();
        digest.update(snapshot.revision.as_bytes());
        for (id, entry_id) in journal_revisions {
            digest.update(id.as_bytes());
            digest.update(entry_id.to_le_bytes());
        }
        for (id, summary) in event_revisions {
            digest.update(id.as_bytes());
            digest.update(summary.state.as_bytes());
            digest.update(summary.delivered.to_le_bytes());
            digest.update(summary.refused.to_le_bytes());
            digest.update(summary.last_event_at_ms.unwrap_or_default().to_le_bytes());
            if let Some(reason) = summary.reason {
                digest.update(reason.as_bytes());
            }
        }
        snapshot.revision = format!("{:x}", digest.finalize());
    }
    for job in jobs {
        let Some(node) = snapshot.nodes.iter_mut().find(|node| {
            node.pointer("/identity/id").and_then(Value::as_str) == Some(job.id.as_str())
                && node.pointer("/identity/kind").and_then(Value::as_str) == Some("sentinel")
        }) else {
            continue;
        };
        let entries = journals.get(&job.id).map(Vec::as_slice).unwrap_or_default();
        let latest_run = entries.first().map(|entry| entry.run_id.as_str());
        let run_entries = latest_run
            .map(|run_id| {
                entries
                    .iter()
                    .filter(|entry| entry.run_id == run_id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let close = run_entries.iter().find(|entry| {
            entry.step_name == "$run"
                && matches!(
                    entry.outcome.as_deref(),
                    Some("done" | "failed" | "skipped")
                )
        });
        let current_step = if latest_run.is_none() || close.is_some() {
            None
        } else {
            run_entries
                .iter()
                .find(|entry| entry.state == "running" && entry.step_index >= 0)
                .map(|entry| entry.step_index as usize)
                .or_else(|| {
                    job.pipeline.as_ref().and_then(|spec| {
                        (0..spec.steps.len()).find(|index| {
                            !run_entries.iter().any(|entry| {
                                entry.step_index == *index as i64
                                    && matches!(entry.state.as_str(), "done" | "failed" | "skipped")
                            })
                        })
                    })
                })
        };
        let steps = if let Some(spec) = &job.pipeline {
            spec.steps
                .iter()
                .enumerate()
                .map(|(index, step)| {
                    let accepted = step_outcomes(&step.kind);
                    let journal = run_entries
                        .iter()
                        .find(|entry| {
                            entry.step_index == index as i64
                                && entry
                                    .outcome
                                    .as_ref()
                                    .is_some_and(|value| accepted.contains(value))
                        })
                        .or_else(|| {
                            run_entries.iter().find(|entry| {
                                entry.step_index == index as i64
                                    && matches!(entry.state.as_str(), "running" | "skipped")
                            })
                        });
                    let summary = run_entries.iter().find(|entry| {
                        entry.step_index == index as i64 && entry.result_summary.is_some()
                    });
                    pipeline_step_view(index, &step.name, step_kind(&step.kind), journal, summary)
                })
                .collect::<Vec<_>>()
        } else {
            legacy_step_views(job)
        };
        let trigger = job
            .pipeline
            .as_ref()
            .map(|spec| trigger_view(&spec.trigger))
            .unwrap_or_else(|| legacy_trigger_view(job));
        let target = job
            .pipeline
            .as_ref()
            .map(|spec| result_target_view(&spec.result.to, spec.owner, job))
            .unwrap_or_else(|| legacy_result_target_view(job));
        let result_entry = run_entries
            .iter()
            .find(|entry| entry.step_name == "$result");
        let result_state = if job.pipeline.is_some() {
            match result_entry.and_then(|entry| entry.outcome.as_deref()) {
                Some("handler_result") => "delivered",
                Some("handler_unconfirmed" | "handler_failed") => "failed",
                _ if result_entry.is_some_and(|entry| entry.state == "running") => "delivering",
                _ => "waiting",
            }
        } else {
            legacy_result_state(job)
        };
        let result_reason = if job.pipeline.is_some() {
            result_entry.and_then(|entry| entry.reason.clone())
        } else {
            legacy_result_reason(job)
        };
        let result_body_ref = result_entry
            .and_then(|entry| entry.result_summary.as_deref())
            .and_then(|summary| summary.strip_prefix("body_ref:"))
            .filter(|reference| *reference != "unavailable")
            .map(str::to_string);
        let last_event = entries
            .iter()
            .map(|entry| entry.ended_at_ms.unwrap_or(entry.started_at_ms))
            .max();
        let run_state = if matches!(
            job.last_reason.as_deref(),
            Some(
                "uncertain_pipeline_claim"
                    | "uncertain_pipeline_effect"
                    | "uncertain_pipeline_result"
            )
        ) {
            "held"
        } else if job.machine.stopped {
            "stopped"
        } else if job.machine.paused || job.machine.held.is_some() {
            "held"
        } else if close.is_some_and(|entry| entry.outcome.as_deref() == Some("failed")) {
            "failed"
        } else if live_leases.contains(&job.id)
            || run_entries.iter().any(|entry| entry.state == "running")
        {
            "running"
        } else if latest_run.is_some() && close.is_none() {
            "held"
        } else {
            "idle"
        };
        let (owner_kind, owner_tab, owner_team) = match job
            .pipeline
            .as_ref()
            .map(|spec| spec.owner)
            .unwrap_or(codeterm_types::blueprint_pipeline::BlueprintOwner::Tab)
        {
            codeterm_types::blueprint_pipeline::BlueprintOwner::Tab => {
                ("tab", Some(job.creator_tab_id.as_str()), None)
            }
            codeterm_types::blueprint_pipeline::BlueprintOwner::Team => {
                ("team", None, job.team_id.as_deref())
            }
        };
        node["blueprint"] = json!({
            "id":job.id,
            "name":job.name,
            "owner":{"kind":owner_kind,"tab_id":owner_tab,"team_id":owner_team},
            "state":run_state,
            "run_id":latest_run,
            "current_step":current_step,
            "last_event_at_ms":last_event
        });
        node["trigger"] = trigger;
        if job.pipeline.as_ref().is_some_and(|spec| {
            matches!(
                &spec.trigger,
                codeterm_types::blueprint_pipeline::Trigger::Event { .. }
            )
        }) {
            let subscription = event_subscriptions.summary(&job.id);
            node["event_subscription"] = json!({
                "state":subscription.as_ref().map(|item| item.state.as_str()).unwrap_or(if job.machine.stopped { "stopped" } else { "unavailable" }),
                "delivered":subscription.as_ref().map(|item| item.delivered).unwrap_or_default(),
                "refused":subscription.as_ref().map(|item| item.refused).unwrap_or_default(),
                "last_event_at_ms":subscription.as_ref().and_then(|item| item.last_event_at_ms),
                "reason":subscription.as_ref().and_then(|item| item.reason.clone()).or_else(|| (!job.machine.stopped).then(|| "not_subscribed".into()))
            });
        }
        node["steps"] = json!(steps);
        node["result"] = json!({
            "target":target,
            "state":result_state,
            "reason":result_reason,
            "body_ref":result_body_ref
        });
    }
}

fn pipeline_step_view(
    index: usize,
    name: &str,
    kind: &str,
    entry: Option<&&BlueprintJournalEntry>,
    summary: Option<&&BlueprintJournalEntry>,
) -> Value {
    let state = entry.map_or("waiting", |entry| entry.state.as_str());
    json!({
        "index":index,
        "name":name,
        "kind":kind,
        "title":format!("{}: {name}", kind),
        "state":state,
        "outcome":entry.filter(|entry| !entry.outcome.as_deref().is_some_and(|outcome| outcome.starts_with("skip:"))).and_then(|entry| entry.outcome.clone()),
        "started_at_ms":entry.map(|entry| entry.started_at_ms),
        "ended_at_ms":entry.and_then(|entry| entry.ended_at_ms),
        "result_summary":summary.and_then(|entry| entry.result_summary.clone()),
        "reason":entry.and_then(|entry| entry.reason.clone())
    })
}

fn step_kind(kind: &codeterm_types::blueprint_pipeline::StepKind) -> &'static str {
    use codeterm_types::blueprint_pipeline::StepKind;
    match kind {
        StepKind::Command { .. } => "command",
        StepKind::Convert { .. } => "convert",
        StepKind::Ask { .. } => "ask",
        StepKind::Filter { .. } => "filter",
    }
}

fn step_outcomes(kind: &codeterm_types::blueprint_pipeline::StepKind) -> BTreeSet<String> {
    use codeterm_types::blueprint_pipeline::StepKind;
    match kind {
        StepKind::Command { .. } | StepKind::Convert { .. } => {
            BTreeSet::from(["done".into(), "failed".into()])
        }
        StepKind::Ask { buckets, .. } => buckets
            .iter()
            .cloned()
            .chain(std::iter::once("other".into()))
            .collect(),
        StepKind::Filter { bucket, .. } => bucket
            .iter()
            .cloned()
            .chain(["done".into(), "failed".into()])
            .collect(),
    }
}

fn trigger_view(trigger: &codeterm_types::blueprint_pipeline::Trigger) -> Value {
    use codeterm_types::blueprint_pipeline::Trigger;
    match trigger {
        Trigger::Schedule { every } => {
            json!({"type":"schedule","summary":format!("every {every}")})
        }
        Trigger::Event { kind, .. } => {
            json!({"type":"event","summary":format!("event {kind}")})
        }
        Trigger::Input { kind, source } => {
            json!({"type":"input", "summary":input_summary(*kind, source)})
        }
    }
}

fn input_summary(
    kind: codeterm_types::sentinel_input::MediaKind,
    source: &codeterm_types::sentinel_input::DataRef,
) -> String {
    use codeterm_types::sentinel_input::DataRef;
    let (ref_type, display) = match source {
        DataRef::Text(_) => ("text", "inline text".into()),
        DataRef::File(path) => {
            let name = std::path::Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            ("file", name.into_owned())
        }
        DataRef::Url(url) => (
            "url",
            url.split_once("://")
                .and_then(|(_, rest)| {
                    rest.split(|character| matches!(character, '/' | '?' | '#'))
                        .next()
                })
                .map(|authority| authority.rsplit('@').next().unwrap_or("source"))
                .unwrap_or("source")
                .to_string(),
        ),
    };
    format!("{} {ref_type} {display}", kind.as_str())
}

fn legacy_trigger_view(job: &Sentinel) -> Value {
    if let Some(input) = &job.input {
        let source = graph::source_descriptor(job);
        return json!({"type":"input","summary":format!(
            "{} {} {}",
            input.kind.as_str(),
            source["ref_type"].as_str().unwrap_or("source"),
            source["display_name"].as_str().unwrap_or_default()
        ).trim()});
    }
    if let Some(binding) = &job.observe {
        let source = match binding {
            crate::sentinel::source::TabBinding::Tab(id) => format!("tab {id}"),
            crate::sentinel::source::TabBinding::CronRun(id) => format!("cron run {id}"),
        };
        return json!({"type":"event","summary":format!("observe {source}")});
    }
    json!({"type":"schedule","summary":format!("every {}ms", job.interval_ms)})
}

fn legacy_step_views(job: &Sentinel) -> Vec<Value> {
    let source = if job.observe.is_some() {
        "observe"
    } else {
        "command"
    };
    let accepted_at = job
        .transitions
        .last()
        .map(|transition| transition.accepted_at);
    let state = if accepted_at.is_some() {
        "done"
    } else {
        "waiting"
    };
    let mut steps = vec![json!({
        "index":0,"name":source,"kind":"command","title":if source == "observe" {"Observe"} else {"Command"},
        "state":state,"outcome":if accepted_at.is_some(){Some("done")}else{None},
        "started_at_ms":accepted_at,"ended_at_ms":accepted_at,
        "result_summary":accepted_at.map(|_| "legacy stage result"),"reason":Value::Null
    })];
    steps.push(json!({
        "index":1,"name":"classify","kind":"ask","title":"Classify buckets",
        "state":state,"outcome":job.transitions.last().map(|transition| transition.bucket.as_str()),
        "started_at_ms":accepted_at,"ended_at_ms":accepted_at,
        "result_summary":accepted_at.map(|_| "legacy bucket classified"),"reason":Value::Null
    }));
    steps
}

fn result_target_view(
    target: &codeterm_types::blueprint_pipeline::ResultTarget,
    owner: codeterm_types::blueprint_pipeline::BlueprintOwner,
    job: &Sentinel,
) -> Value {
    use codeterm_types::blueprint_pipeline::{BlueprintOwner, ResultTarget};
    match target {
        ResultTarget::Owner if owner == BlueprintOwner::Team => {
            json!({"kind":"team","id":job.team_id})
        }
        ResultTarget::Owner => json!({"kind":"owner_tab","id":job.creator_tab_id}),
        ResultTarget::Tab(id) => json!({"kind":"tab","id":id}),
        ResultTarget::Team => json!({"kind":"team","id":job.team_id}),
        ResultTarget::Notify => json!({"kind":"notify","id":Value::Null}),
    }
}

fn legacy_result_state(job: &Sentinel) -> &'static str {
    use crate::sentinel::handlers::Status;
    let Some(transition) = job.transitions.last() else {
        return "waiting";
    };
    if transition.handlers.is_empty() {
        return "waiting";
    }
    if transition
        .handlers
        .iter()
        .all(|handler| matches!(&handler.status, Status::Completed | Status::Noop))
    {
        "delivered"
    } else if transition
        .handlers
        .iter()
        .any(|handler| matches!(&handler.status, Status::Unconfirmed))
    {
        "delivering"
    } else if transition
        .handlers
        .iter()
        .any(|handler| matches!(&handler.status, Status::Failed | Status::Skipped))
    {
        "failed"
    } else {
        "delivering"
    }
}

fn legacy_result_reason(job: &Sentinel) -> Option<String> {
    use crate::sentinel::handlers::Status;
    let transition = job.transitions.last()?;
    transition
        .handlers
        .iter()
        .find_map(|handler| match &handler.status {
            Status::Unconfirmed => Some("effect_unconfirmed".into()),
            Status::Failed => Some("handler_failed".into()),
            Status::Skipped => Some("handler_skipped".into()),
            Status::Completed | Status::Noop => None,
        })
}

fn legacy_result_target_view(job: &Sentinel) -> Value {
    let route = job.routes.values().flatten().next();
    match route.map(|handler| &handler.action) {
        Some(Route::Send(id)) => json!({"kind":"tab","id":id}),
        Some(Route::Notify) | None => json!({"kind":"owner_tab","id":job.creator_tab_id}),
        _ if job.team_id.is_some() => json!({"kind":"team","id":job.team_id}),
        _ => json!({"kind":"owner_tab","id":job.creator_tab_id}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cron::clock::MockClock;
    use crate::sentinel::store::{Create, Owner, Store};
    use codeterm_types::blueprint_pipeline::{
        BlueprintOwner, BlueprintResult, BlueprintSpec, IoType, ResultTarget, Step, StepKind,
        Trigger,
    };
    use std::collections::BTreeMap;

    fn owner() -> Owner {
        Owner {
            creator_tab_id: "creator".into(),
            team_id: Some("team".into()),
            workspace_id: "workspace".into(),
            cwd: ".".into(),
        }
    }

    fn project(job: &Sentinel) -> Value {
        project_snapshot(job, &BTreeMap::new()).nodes.remove(0)
    }

    fn project_snapshot(
        job: &Sentinel,
        journals: &BTreeMap<String, Vec<BlueprintJournalEntry>>,
    ) -> Snapshot {
        let mut snapshot = Snapshot {
            workspace_id: job.workspace_id.clone(),
            team_id: job.team_id.clone(),
            all_teams: false,
            revision: "revision".into(),
            scenarios: Vec::new(),
            nodes: vec![json!({"identity":graph::identity(job)})],
            edges: Vec::new(),
            truncated: false,
            configured_fallback: Value::Null,
        };
        project_pipeline_contract(
            &mut snapshot,
            std::slice::from_ref(job),
            journals,
            &BTreeSet::new(),
            &codeterm_plugin::graph_events::SubscriptionTable::new(),
        );
        snapshot
    }

    #[test]
    fn blueprint_pipeline_read_model_projects_legacy_stage_as_pipeline() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(
            directory.path().join("blueprints.json"),
            MockClock::starting_at(1000),
        );
        let job = store
            .prepare(
                Create {
                    name: Some("legacy stage".into()),
                    probe: Some("printf ready".into()),
                    observe: None,
                    input: None,
                    question: "ready?".into(),
                    buckets: vec!["done".into()],
                    every: "20s".into(),
                    routes: Default::default(),
                    matches: BTreeMap::from([("done".into(), ".*".into())]),
                    rearm_on_change: Default::default(),
                    until: None,
                    timeout: None,
                    paused: false,
                    handlers: None,
                    escalation_timeout_ms: None,
                    max_transitions: Some(10),
                    max_actions: Some(10),
                },
                owner(),
                true,
            )
            .unwrap();

        let node = project(&job);
        assert_eq!(node["blueprint"]["id"], job.id);
        assert_eq!(node["blueprint"]["owner"]["kind"], "tab");
        assert_eq!(node["blueprint"]["owner"]["tab_id"], "creator");
        assert!(node["blueprint"]["owner"]["team_id"].is_null());
        assert_eq!(node["trigger"]["type"], "schedule");
        assert_eq!(node["steps"].as_array().unwrap().len(), 2);
        assert_eq!(node["steps"][0]["name"], "command");
        assert_eq!(node["steps"][1]["kind"], "ask");
        assert_eq!(node["result"]["state"], "waiting");
        assert_eq!(
            input_summary(
                codeterm_types::sentinel_input::MediaKind::Audio,
                &codeterm_types::sentinel_input::DataRef::Url(
                    "https://user:secret@example.com/path?token=secret".into()
                )
            ),
            "audio url example.com"
        );
    }

    #[test]
    fn blueprint_pipeline_team_owner_defaults_result_target_to_team() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(
            directory.path().join("blueprints.json"),
            MockClock::starting_at(1000),
        );
        let spec = BlueprintSpec {
            name: "team pipeline".into(),
            owner: BlueprintOwner::Team,
            trigger: Trigger::Schedule {
                every: "20s".into(),
            },
            steps: vec![Step {
                name: "emit".into(),
                input: IoType::Text,
                kind: StepKind::Command {
                    command: "printf ready".into(),
                },
                on: Default::default(),
            }],
            result: BlueprintResult {
                to: ResultTarget::Owner,
            },
        };
        let job = store.prepare_pipeline(spec.clone(), owner(), true).unwrap();

        let node = project(&job);
        assert_eq!(node["blueprint"]["owner"]["kind"], "team");
        assert!(node["blueprint"]["owner"]["tab_id"].is_null());
        assert_eq!(node["blueprint"]["owner"]["team_id"], "team");
        assert_eq!(node["result"]["target"]["kind"], "team");
        assert_eq!(node["result"]["target"]["id"], "team");
    }

    #[test]
    fn blueprint_pipeline_read_model_revision_tracks_journal_rows() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(
            directory.path().join("blueprints.json"),
            MockClock::starting_at(1000),
        );
        let spec = BlueprintSpec {
            name: "revision pipeline".into(),
            owner: BlueprintOwner::Tab,
            trigger: Trigger::Schedule {
                every: "20s".into(),
            },
            steps: vec![Step {
                name: "emit".into(),
                input: IoType::Text,
                kind: StepKind::Command {
                    command: "echo ready".into(),
                },
                on: Default::default(),
            }],
            result: BlueprintResult {
                to: ResultTarget::Owner,
            },
        };
        let job = store.prepare_pipeline(spec, owner(), true).unwrap();
        let before = project_snapshot(&job, &BTreeMap::new()).revision;
        let journals = BTreeMap::from([(
            job.id.clone(),
            vec![BlueprintJournalEntry {
                entry_id: 17,
                run_id: "run_revision".into(),
                step_index: 0,
                step_name: "emit".into(),
                state: "done".into(),
                outcome: Some("done".into()),
                started_at_ms: 1000,
                ended_at_ms: Some(1100),
                result_summary: Some("step completed".into()),
                reason: None,
            }],
        )]);

        let after = project_snapshot(&job, &journals).revision;
        assert_ne!(before, after);
    }

    #[test]
    fn blueprint_delivery_read_model_projects_reason_and_body_reference() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(
            directory.path().join("blueprints.json"),
            MockClock::starting_at(1000),
        );
        let spec = BlueprintSpec {
            name: "result delivery".into(),
            owner: BlueprintOwner::Tab,
            trigger: Trigger::Schedule {
                every: "20s".into(),
            },
            steps: vec![Step {
                name: "emit".into(),
                input: IoType::Text,
                kind: StepKind::Command {
                    command: "echo ready".into(),
                },
                on: Default::default(),
            }],
            result: BlueprintResult {
                to: ResultTarget::Owner,
            },
        };
        let job = store.prepare_pipeline(spec, owner(), true).unwrap();
        let journal = BlueprintJournalEntry {
            entry_id: 17,
            run_id: "run_result".into(),
            step_index: -1,
            step_name: "$result".into(),
            state: "failed".into(),
            outcome: Some("handler_unconfirmed".into()),
            started_at_ms: 1000,
            ended_at_ms: Some(1100),
            result_summary: Some("body_ref:inbox/result.md".into()),
            reason: Some("delivery_pending".into()),
        };
        let node = project_snapshot(&job, &BTreeMap::from([(job.id.clone(), vec![journal])]))
            .nodes
            .remove(0);

        assert_eq!(node["result"]["state"], "failed");
        assert_eq!(node["result"]["reason"], "delivery_pending");
        assert_eq!(node["result"]["body_ref"], "inbox/result.md");
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
                    .filter(|j| {
                        (j.agent_authored || j.pipeline.is_some())
                            && j.scenario_id.as_deref() == Some(j.id.as_str())
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
                crate::api::events::emit_everywhere(&app, "automation-graph-changed", change);
            }
        });
    }
}
