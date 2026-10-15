mod detail;
pub(crate) mod files;
pub use files::FileReference;
mod history;
mod runs;
pub use codeterm_sentinel::identity::{identity, scope_revision, Identity};
pub(crate) use detail::configured_fallback;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Serialize;
use serde_json::{json, Value};

use super::{
    handlers::ResultRecord,
    machine::Route,
    source::TabBinding,
    storage::Staged,
    store::{Sentinel, Store},
};
use crate::cron::store::{CronJob, CronRunInfo};
use crate::multiplexer::{state::MultiplexerState, types::Tab};
use crate::service::supporter::QuestionAnswer;

pub const MAX_NODES: usize = 1024;
pub const MAX_EDGES: usize = 4096;
pub const MAX_RUNS_PER_CRON: usize = 64;

#[derive(Clone, Debug, Serialize)]
pub struct Scenario {
    pub id: String,
    pub name: String,
    pub root: Identity,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub workspace_id: String,
    pub team_id: Option<String>,
    pub all_teams: bool,
    pub revision: String,
    pub scenarios: Vec<Scenario>,
    pub nodes: Vec<Value>,
    pub runs: Vec<Value>,
    pub blueprints: Vec<Value>,
    pub edges: Vec<Value>,
    pub truncated: bool,
    pub configured_fallback: Value,
}

#[derive(Clone)]
pub struct ConnectedCron {
    pub job: Option<CronJob>,
    pub identity: Identity,
    pub runs: Vec<CronRunInfo>,
}

pub fn validate_scope(
    mux: &MultiplexerState,
    workspace: &str,
    team: Option<&str>,
) -> Result<(), String> {
    if mux.get_workspace(workspace).is_none() {
        return Err("automation workspace not found".into());
    }
    if let Some(team) = team {
        if mux.get_team(team).is_none() || mux.team_workspace(team).as_deref() != Some(workspace) {
            return Err("automation Team does not belong to the current workspace".into());
        }
    }
    Ok(())
}

#[cfg(test)]
pub fn scoped_inventory(
    store: &super::store::Store,
    mux: &MultiplexerState,
    workspace: &str,
    team: Option<&str>,
) -> Result<Vec<Sentinel>, String> {
    validate_scope(mux, workspace, team)?;
    store.ensure_available()?;
    Ok(store
        .list()
        .iter()
        .filter(|job| job.workspace_id == workspace && job.team_id.as_deref() == team)
        .cloned()
        .collect())
}

pub fn reachable_ids(jobs: &[Sentinel]) -> BTreeSet<String> {
    let mut pending: Vec<_> = jobs
        .iter()
        .filter(|job| job.is_scenario_root())
        .map(|job| job.id.clone())
        .collect();
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(source) = jobs.iter().find(|job| job.id == id) else {
            continue;
        };
        pending.extend(source.routes.values().flatten().filter_map(|handler| {
            if let Route::Sentinel(target) = &handler.action {
                jobs.iter()
                    .find(|job| job.id == *target && job.same_owner(source))
                    .map(|job| job.id.clone())
            } else {
                None
            }
        }));
    }
    seen
}

pub fn host_delete_staged(
    store: &mut Store,
    mux: &MultiplexerState,
    workspace: &str,
    team: Option<&str>,
    id: &str,
) -> Result<Staged<()>, String> {
    validate_scope(mux, workspace, team)?;
    store.ensure_available()?;
    let root = store.get(id).ok_or("Blueprint scenario not found")?;
    if root.workspace_id != workspace
        || root.team_id.as_deref() != team
        || !root.is_scenario_root()
        || !reachable_ids(store.list()).contains(id)
    {
        return Err("Blueprint scenario is unavailable in this automation scope".into());
    }
    Ok(store.stage_delete_scenario(id, workspace, team))
}

fn safe_name(text: &str) -> String {
    QuestionAnswer::new(text, "", "").name
}
fn safe_purpose(text: &str) -> String {
    QuestionAnswer::new("", text, "").question
}

/// The node source shape: probe, observe kind, or input metadata without inline text,
/// path directories or the URL query.
pub(crate) fn source_descriptor(job: &Sentinel) -> Value {
    if job.pipeline().is_some() {
        return json!({"type":"pipeline"});
    }
    if let Some(input) = &job.input().cloned() {
        use codeterm_blueprint_types::sentinel_input::DataRef;
        let ref_type = match &input.data {
            DataRef::Text(_) => "text",
            DataRef::File(_) => "file",
            DataRef::Url(_) => "url",
        };
        return json!({
            "type": "input",
            "kind": input.kind.as_str(),
            "ref_type": ref_type,
            "display_name": codeterm_blueprint_types::sentinel_input::display_name(&input.data),
            "convert_to": input.convert_to.map(|kind| kind.as_str()),
        });
    }
    if let Some(binding) = &job.observe().cloned() {
        if let Some(terminal) = &job.terminal().cloned() {
            return json!({"type":"terminal","kind":"tab","pattern":terminal.pattern,"idle_ms":terminal.idle_ms,"max_payload_bytes":codeterm_blueprint_types::blueprint_terminal::MAX_PAYLOAD_BYTES});
        }
        let kind = match binding {
            TabBinding::Tab(_) => "tab",
            TabBinding::CronRun(_) => "cron_run",
        };
        return json!({"type": "observe", "kind": kind});
    }
    json!({"type": "probe"})
}

fn record(result: &ResultRecord) -> Value {
    let mut value = json!({"status": result.status, "execution_id":result.execution_id,"accepted_at":result.accepted_at});
    if let Some(command) = &result.command_result {
        value["command_result"] = json!(command);
    }
    value
}

fn placeholder(identity: &Identity) -> Value {
    let mut node = json!({"identity":identity,"name":format!("Missing {}", identity.kind),"purpose":null,"state":null,"availability":"missing","hidden":null,"schedule":null,"cadence_ms":null,"expires_at":null,"outcomes":[],"last_result":null,"stop":{"allowed":false,"reason":"exact target unavailable"},"executor":null});
    if identity.kind == "blueprint" {
        node["detail"] = detail::missing(identity);
    }
    node
}

pub fn tab_identity(tab: &Tab, workspace: &str) -> Identity {
    Identity {
        kind: "worker".into(),
        id: tab.id.clone(),
        workspace_id: workspace.into(),
        team_id: tab.team_id.clone(),
        owner_tab_id: tab.spawned_by_tab_id.clone(),
    }
}

fn worker(tab: &Tab, identity: &Identity) -> Value {
    let configured = tab.agent_requested_model.clone();
    let reported = tab.agent_session_model.clone();
    json!({"identity":identity,"name":safe_name(&tab.title),"purpose":tab.description.as_deref().map(safe_purpose),"state":if tab.agent_booting{Some("launching")}else{None},"state_source":"tab_lifecycle","availability":"available","hidden":tab.hidden,"schedule":null,"cadence_ms":null,"expires_at":null,"outcomes":[],"last_result":null,"stop":{"allowed":false,"reason":"graph stop applies only to Blueprints"},"executor":{"kind":"agent","selection":if configured.is_some(){"explicit"}else{"unknown"},"provider_id":tab.agent_type,"adapter_id":null,"model":reported.as_ref().or(configured.as_ref()),"configured_model":configured,"reported_model":reported,"capabilities":null}})
}

pub fn project_worker_states(
    snapshot: &mut Snapshot,
    mux: &MultiplexerState,
    resolve: impl Fn(&Tab) -> Option<crate::agent::status::TabStatus>,
) {
    for node in &mut snapshot.nodes {
        if node["identity"]["kind"] != "worker" || node["availability"] != "available" {
            continue;
        }
        let Ok(exact) = serde_json::from_value::<Identity>(node["identity"].clone()) else {
            continue;
        };
        let Some(tab) = mux.get_tab(&exact.id).filter(|tab| {
            tab_identity(tab, &exact.workspace_id) == exact
                && mux.find_workspace_for_tab(&exact.id).as_deref()
                    == Some(exact.workspace_id.as_str())
        }) else {
            continue;
        };
        if let Some(status) = resolve(tab) {
            if node["state_source"] == "cron_run" {
                node["cron_run_state"] = node["state"].clone();
            }
            node["state"] = json!(status.label());
            node["state_source"] = json!("tab_status");
        } else if tab.agent_state.is_some() {
            node["state"] =
                json!(crate::multiplexer::state::effective_tab_status(tab, None).label());
            node["state_source"] = json!("provider_state");
        }
    }
}

fn target(
    source: &Sentinel,
    action: &Route,
    recorded: Option<&Identity>,
    jobs: &[Sentinel],
    mux: &MultiplexerState,
    crons: &BTreeMap<String, ConnectedCron>,
) -> Option<(Identity, bool)> {
    let (kind, id, owner) = match action {
        Route::Sentinel(id) => ("blueprint", id, Some(source.creator_tab_id.clone())),
        Route::Send(id) => ("worker", id, None),
        Route::Escalate(super::machine::EscalateTarget::Agent(id)) => ("worker", id, None),
        Route::Cron(id) => ("cron", id, None),
        Route::Notify
        | Route::Stop
        | Route::Command(_)
        | Route::RespondPrompt(_)
        | Route::Escalate(super::machine::EscalateTarget::Creator) => return None,
    };
    let unknown = Identity {
        kind: kind.into(),
        id: id.clone(),
        workspace_id: source.workspace_id.clone(),
        team_id: source.team_id.clone(),
        owner_tab_id: owner,
    };
    let actual = match action {
        Route::Sentinel(id) => jobs
            .iter()
            .find(|j| j.id == *id && j.same_owner(source))
            .map(identity),
        Route::Send(id) | Route::Escalate(super::machine::EscalateTarget::Agent(id)) => mux
            .get_tab(id)
            .filter(|tab| {
                (matches!(action, Route::Send(_)) || tab.agent_type.is_some())
                    && tab.team_id == source.team_id
                    && mux.find_workspace_for_tab(id).as_deref() == Some(&source.workspace_id)
            })
            .map(|tab| tab_identity(tab, &source.workspace_id)),
        Route::Cron(id) => crons
            .get(id)
            .filter(|cron| {
                cron.job.is_some()
                    && cron.identity.team_id == source.team_id
                    && cron.identity.workspace_id == source.workspace_id
            })
            .map(|cron| cron.identity.clone()),
        _ => None,
    };
    let expected = recorded
        .cloned()
        .unwrap_or_else(|| actual.clone().unwrap_or(unknown));
    let available = actual.as_ref() == Some(&expected);
    Some((expected, available))
}

fn add_cron(
    exact: &Identity,
    cron: &ConnectedCron,
    mux: &MultiplexerState,
    nodes: &mut BTreeMap<Identity, Value>,
    edges: &mut Vec<Value>,
    worker_statuses: &mut BTreeMap<Identity, u64>,
    truncated: &mut bool,
) {
    nodes.entry(exact.clone()).or_insert_with(|| match &cron.job {
        Some(job) => json!({"identity":exact,"name":safe_name(&job.name),"purpose":job.description.as_deref().map(safe_purpose),"state":if job.enabled{"enabled"}else{"disabled"},"availability":"available","hidden":null,"schedule":safe_purpose(&job.schedule),"cadence_ms":null,"expires_at":null,"outcomes":[],"last_result":null,"stop":{"allowed":false,"reason":"graph stop applies only to Blueprints"},"executor":null}),
        None => placeholder(exact),
    });
    for run in cron.runs.iter().take(MAX_RUNS_PER_CRON) {
        let Some(tab_id) = &run.tab_id else { continue };
        if edges.len() >= MAX_EDGES {
            *truncated = true;
            break;
        }
        let tab = mux.get_tab(tab_id).filter(|tab| {
            tab.agent_type.is_some()
                && tab.team_id == exact.team_id
                && tab.spawned_by_tab_id == exact.owner_tab_id
                && mux.find_workspace_for_tab(tab_id).as_deref()
                    == Some(exact.workspace_id.as_str())
        });
        let worker_id = tab
            .map(|tab| tab_identity(tab, &exact.workspace_id))
            .unwrap_or(Identity {
                kind: "worker".into(),
                id: tab_id.clone(),
                workspace_id: exact.workspace_id.clone(),
                team_id: exact.team_id.clone(),
                owner_tab_id: exact.owner_tab_id.clone(),
            });
        if nodes.len() >= MAX_NODES && !nodes.contains_key(&worker_id) {
            *truncated = true;
            continue;
        }
        let node = nodes.entry(worker_id.clone()).or_insert_with(|| {
            tab.map(|tab| worker(tab, &worker_id))
                .unwrap_or_else(|| placeholder(&worker_id))
        });
        if worker_statuses
            .get(&worker_id)
            .is_none_or(|started| *started < run.started_at)
        {
            node["state"] = json!(format!("{:?}", run.status).to_lowercase());
            node["state_source"] = json!("cron_run");
            worker_statuses.insert(worker_id.clone(), run.started_at);
        }
        let edge_id = format!("{}:{}:{}", exact.id, run.run_id, tab_id);
        if !edges.iter().any(|edge| edge["id"] == edge_id) {
            edges.push(json!({"id":edge_id,"source":exact,"target":worker_id,"bucket":"run","handler_id":run.run_id,"handler_type":"cron","result":{"status":format!("{:?}",run.status).to_lowercase(),"execution_id":run.run_id,"accepted_at":run.started_at},"evidence":{"kind":"observed","execution_id":run.run_id}}));
        }
    }
    *truncated |= cron.runs.len() > MAX_RUNS_PER_CRON;
}

pub fn project(
    workspace: &str,
    team: Option<&str>,
    jobs: &[Sentinel],
    mux: &MultiplexerState,
    crons: &BTreeMap<String, ConnectedCron>,
    configured_fallback: Value,
    now_ms: u64,
    live_leases: &BTreeSet<String>,
) -> Result<Snapshot, String> {
    validate_scope(mux, workspace, team)?;
    let mut scenarios: Vec<_> = jobs
        .iter()
        .filter(|j| {
            j.is_scenario_root() && j.workspace_id == workspace && j.team_id.as_deref() == team
        })
        .map(|job| Scenario {
            id: job.id.clone(),
            name: safe_name(job.name.as_deref().unwrap_or(&job.id)),
            root: identity(job),
        })
        .collect();
    scenarios.sort_by(|a, b| a.id.cmp(&b.id));
    let mut pending: VecDeque<_> = scenarios.iter().map(|s| s.root.clone()).collect();
    let mut nodes: BTreeMap<_, _> = scenarios
        .iter()
        .map(|scenario| (scenario.root.clone(), placeholder(&scenario.root)))
        .collect();
    let mut worker_statuses = BTreeMap::new();
    let mut edges = Vec::new();
    let mut visited = BTreeSet::new();
    let mut truncated = false;
    let mut history_budget = 4096;
    while let Some(exact) = pending.pop_front() {
        if !visited.insert(exact.clone()) {
            continue;
        }
        if nodes.len() >= MAX_NODES && !nodes.contains_key(&exact) {
            truncated = true;
            continue;
        }
        nodes
            .entry(exact.clone())
            .or_insert_with(|| placeholder(&exact));
        let Some(job) = jobs.iter().find(|j| identity(j) == exact) else {
            continue;
        };
        let expired = job.expires_at.is_some_and(|at| now_ms >= at);
        let executing = live_leases.contains(&job.id);
        let last = job.transitions.last();
        let mut outcomes = Vec::new();
        for bucket in &job.buckets {
            let mut handlers = Vec::new();
            for handler in job.routes.get(bucket).into_iter().flatten() {
                let result = job
                    .transitions
                    .iter()
                    .rev()
                    .filter(|t| t.bucket == *bucket)
                    .flat_map(|t| t.handlers.iter().filter(|r| r.handler_id == handler.id))
                    .next()
                    .map(record);
                let target = target(
                    job,
                    &handler.action,
                    handler.target.as_ref(),
                    jobs,
                    mux,
                    crons,
                );
                let mut projected = json!({"id":handler.id,"type":super::routes::action_name(&handler.action),"target":target.as_ref().map(|t| &t.0),"availability":if target.as_ref().is_none_or(|t| t.1){"available"}else{"missing"},"last_result":result});
                if let Route::Command(parameters) = &handler.action {
                    projected["label"] = json!(parameters.label);
                    projected["timeout_ms"] = json!(parameters.timeout_ms);
                }
                handlers.push(projected);
                if let Some((target, available)) = target {
                    if edges.len() >= MAX_EDGES {
                        truncated = true;
                        continue;
                    }
                    edges.push(json!({"id":format!("{}:{}:{}",job.id,bucket,handler.id),"source":exact,"target":target,"bucket":bucket,"handler_id":handler.id,"handler_type":super::routes::action_name(&handler.action),"result":result,"evidence":{"kind":"configured","route_id":handler.id}}));
                    if nodes.len() >= MAX_NODES && !nodes.contains_key(&target) {
                        truncated = true;
                        continue;
                    }
                    if !available {
                        nodes
                            .entry(target.clone())
                            .or_insert_with(|| placeholder(&target));
                        if target.kind == "cron" {
                            if let Some(cron) =
                                crons.get(&target.id).filter(|cron| cron.identity == target)
                            {
                                add_cron(
                                    &target,
                                    cron,
                                    mux,
                                    &mut nodes,
                                    &mut edges,
                                    &mut worker_statuses,
                                    &mut truncated,
                                );
                            }
                        }
                        continue;
                    }
                    match target.kind.as_str() {
                        "blueprint" => pending.push_back(target),
                        "worker" => {
                            if let Some(tab) = mux.get_tab(&target.id) {
                                nodes.insert(target.clone(), worker(tab, &target));
                            }
                        }
                        "cron" => {
                            if let Some(cron) = crons.get(&target.id) {
                                add_cron(
                                    &target,
                                    cron,
                                    mux,
                                    &mut nodes,
                                    &mut edges,
                                    &mut worker_statuses,
                                    &mut truncated,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            outcomes.push(json!({"bucket":bucket,"handlers":handlers}));
        }
        let last_result = last.map(|t| { let mut result = json!({"bucket":t.bucket,"accepted_at":t.accepted_at,"handlers":t.handlers.iter().map(|r| { let mut value=record(r); value["handler_id"]=json!(r.handler_id); value }).collect::<Vec<_>>()}); detail::project_decision(&mut result, t.decision.as_ref()); result });
        let history = history::project(job, &mut history_budget);
        let history_truncated = history.len() < job.transitions.len();
        nodes.insert(exact.clone(),json!({"identity":exact,"name":safe_name(job.name.as_deref().unwrap_or(&job.id)),"purpose":safe_purpose(&job.question),"state":if job.machine.stopped && executing{"finishing"}else if job.machine.stopped{"stopped"}else if expired{"expired"}else if job.machine.paused{"paused"}else{"active"},"availability":"available","hidden":false,"schedule":null,"cadence_ms":job.interval_ms,"conversation":{"enabled":job.session,"exchanges":super::session::usage(jobs,job,now_ms).exchanges,"actions":super::session::usage(jobs,job,now_ms).actions,"safety_limit":codeterm_blueprint_types::blueprint_terminal::SESSION_SAFETY_LIMIT,"rate_limit_per_minute":codeterm_blueprint_types::blueprint_terminal::SESSION_RATE_LIMIT},"expires_at":job.expires_at,"outcomes":outcomes,"last_result":last_result,"history":history,"history_truncated":history_truncated,"stop":{"allowed":executing || (!job.machine.stopped && !expired),"reason":if executing{None}else if job.machine.stopped{job.last_reason.as_deref().map(safe_purpose)}else if expired{Some("deadline elapsed".into())}else{None}},"executor":null,"detail":detail::project(job, mux, workspace, team, now_ms, executing),"source":source_descriptor(job),"blueprint_definition":{"source":"stored_agent_definition","definition_id":job.id,"creator_tab_id":job.creator_tab_id,"definition_version":job.definition_version,"lineage":job.amendments.iter().map(|amendment| json!({"version":amendment.version,"replaced_version":amendment.replaced_version,"author_tab_id":amendment.author_tab_id,"cause_transition_id":amendment.cause_transition_id,"at":amendment.at})).collect::<Vec<_>>()}}));
    }
    let nodes: Vec<_> = nodes.into_values().collect();
    let runs = runs::project(&nodes, jobs, now_ms, live_leases);
    Ok(Snapshot {
        blueprints: Vec::new(),
        workspace_id: workspace.into(),
        team_id: team.map(str::to_owned),
        all_teams: false,
        revision: scope_revision(jobs, workspace, team),
        scenarios,
        nodes,
        runs,
        edges,
        truncated,
        configured_fallback,
    })
}
