mod detail;
mod mux;
pub(crate) use mux::MuxSnapshot;
pub use mux::MuxView;
pub(crate) mod files;
pub use files::FileReference;
mod history;
mod runs;
pub use codeterm_sentinel::identity::{identity, scope_revision, Identity};
pub(crate) use detail::configured_fallback;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use codeterm_sentinel::graph_contracts::{
    AgentBlueprintDefinition, Availability, Conversation, DefinitionLineage, Edge, EdgeEvidence,
    Executor, Handler as GraphHandler, HandlerResult, LastResult, Node, Outcome, SourceDescriptor,
    Stop, TransitionHandler,
};

use super::{
    handlers::ResultRecord,
    machine::Route,
    source::TabBinding,
    storage::Staged,
    store::{Sentinel, Store},
};
use codeterm_service_core::cron::store::{CronJob, CronRunInfo};
use codeterm_service_core::multiplexer::{state::MultiplexerState, types::Tab};
use codeterm_service_core::service::supporter::QuestionAnswer;

pub const MAX_NODES: usize = 1024;
pub const MAX_EDGES: usize = 4096;
pub const MAX_RUNS_PER_CRON: usize = 64;

pub use codeterm_sentinel::graph_contracts::Scenario;

pub use codeterm_sentinel::graph_contracts::Snapshot;

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
    validate_view_scope(mux, workspace, team)
}

fn validate_view_scope(
    mux: &dyn MuxView,
    workspace: &str,
    team: Option<&str>,
) -> Result<(), String> {
    if !mux.has_workspace(workspace) {
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

pub(crate) fn is_snapshot_root(job: &Sentinel) -> bool {
    job.scenario_id.is_none() || job.is_scenario_root()
}

fn has_live_owner(job: &Sentinel, mux: &dyn MuxView) -> bool {
    if job.pipeline().is_some_and(|spec| {
        matches!(
            spec.owner,
            codeterm_blueprint_types::blueprint_pipeline::BlueprintOwner::Team
        )
    }) {
        return job.team_id.as_deref().is_some_and(|team| {
            mux.get_team(team).is_some()
                && mux.team_workspace(team).as_deref() == Some(job.workspace_id.as_str())
        });
    }
    mux.get_tab(&job.creator_tab_id).is_some()
        && mux.find_workspace_for_tab(&job.creator_tab_id).as_deref()
            == Some(job.workspace_id.as_str())
}

pub(crate) fn held_reason(job: &Sentinel) -> Option<&str> {
    crate::plan_lock::held_reason(job)
        .or(job.machine.held.as_deref())
        .or_else(|| job.machine.paused.then_some("paused"))
        .or_else(|| {
            job.last_reason
                .as_deref()
                .filter(|reason| reason.starts_with("uncertain_"))
        })
}

pub fn reachable_ids(jobs: &[Sentinel]) -> BTreeSet<String> {
    let mut pending: Vec<_> = jobs
        .iter()
        .filter(|job| is_snapshot_root(job))
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
        || !is_snapshot_root(root)
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
pub(crate) fn source_descriptor(job: &Sentinel) -> SourceDescriptor {
    if job.pipeline().is_some() {
        return SourceDescriptor::Pipeline;
    }
    if let Some(input) = &job.input().cloned() {
        use codeterm_blueprint_types::sentinel_input::DataRef;
        let ref_type = match &input.data {
            DataRef::Text(_) => "text",
            DataRef::File(_) => "file",
            DataRef::Url(_) => "url",
        };
        return SourceDescriptor::Input {
            kind: input.kind.as_str().into(),
            ref_type: ref_type.into(),
            display_name: codeterm_blueprint_types::sentinel_input::display_name(&input.data),
            convert_to: input.convert_to.map(|kind| kind.as_str().into()),
        };
    }
    if let Some(binding) = &job.observe().cloned() {
        if let Some(terminal) = &job.terminal().cloned() {
            return SourceDescriptor::Terminal {
                kind: "tab".into(),
                pattern: terminal.pattern.clone(),
                idle_ms: terminal.idle_ms,
                max_payload_bytes: codeterm_blueprint_types::blueprint_terminal::MAX_PAYLOAD_BYTES,
            };
        }
        let kind = match binding {
            TabBinding::Tab(_) => "tab",
            TabBinding::CronRun(_) => "cron_run",
        };
        return SourceDescriptor::Observe { kind: kind.into() };
    }
    SourceDescriptor::Probe
}

fn record(result: &ResultRecord) -> HandlerResult {
    HandlerResult {
        status: Some(
            match result.status {
                crate::handlers::Status::Unconfirmed => "unconfirmed",
                crate::handlers::Status::Completed => "completed",
                crate::handlers::Status::Failed => "failed",
                crate::handlers::Status::Noop => "noop",
                crate::handlers::Status::Skipped => "skipped",
            }
            .into(),
        ),
        execution_id: result.execution_id.clone(),
        accepted_at: result.accepted_at,
        command_result: result.command_result.clone(),
    }
}

fn placeholder(identity: &Identity) -> Node {
    Node {
        identity: identity.clone(),
        name: format!("Missing {}", identity.kind),
        availability: Availability::Missing,
        stop: Stop {
            allowed: false,
            reason: Some("exact target unavailable".into()),
        },
        detail: (identity.kind == "blueprint").then(|| detail::missing(identity)),
        ..Node::default()
    }
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

fn worker(tab: &Tab, identity: &Identity) -> Node {
    let configured = tab.agent_requested_model.clone();
    let reported = tab.agent_session_model.clone();
    Node {
        identity: identity.clone(),
        name: safe_name(&tab.title),
        purpose: tab.description.as_deref().map(safe_purpose),
        state: tab.agent_booting.then(|| "launching".into()),
        state_source: Some("tab_lifecycle".into()),
        availability: Availability::Available,
        hidden: Some(tab.hidden),
        stop: Stop {
            allowed: false,
            reason: Some("graph stop applies only to Blueprints".into()),
        },
        executor: Some(Executor {
            kind: Some("agent".into()),
            selection: if configured.is_some() {
                "explicit"
            } else {
                "unknown"
            }
            .into(),
            provider_id: tab.agent_type.clone(),
            adapter_id: None,
            model: reported.clone().or(configured.clone()),
            configured_model: configured,
            reported_model: reported,
            capabilities: None,
        }),
        ..Node::default()
    }
}

pub fn project_worker_states(
    snapshot: &mut Snapshot,
    mux: &dyn MuxView,
    resolve: impl Fn(&Tab) -> Option<codeterm_service_core::agent::status::TabStatus>,
) {
    for node in &mut snapshot.nodes {
        if node.identity.kind != "worker" || node.availability != Availability::Available {
            continue;
        }
        let exact = &node.identity;
        let Some(tab) = mux.get_tab(&exact.id).filter(|tab| {
            tab_identity(tab, &exact.workspace_id) == *exact
                && mux.find_workspace_for_tab(&exact.id).as_deref()
                    == Some(exact.workspace_id.as_str())
        }) else {
            continue;
        };
        if let Some(status) = resolve(tab) {
            if node.state_source.as_deref() == Some("cron_run") {
                node.cron_run_state = Some(node.state.clone());
            }
            node.state = Some(status.label().into());
            node.state_source = Some("tab_status".into());
        } else if tab.agent_state.is_some() {
            node.state = Some(
                codeterm_service_core::multiplexer::state::effective_tab_status(tab, None)
                    .label()
                    .into(),
            );
            node.state_source = Some("provider_state".into());
        }
    }
}

fn target(
    source: &Sentinel,
    action: &Route,
    recorded: Option<&Identity>,
    jobs: &[Sentinel],
    mux: &dyn MuxView,
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
    mux: &dyn MuxView,
    nodes: &mut BTreeMap<Identity, Node>,
    edges: &mut Vec<Edge>,
    worker_statuses: &mut BTreeMap<Identity, u64>,
    truncated: &mut bool,
) {
    nodes
        .entry(exact.clone())
        .or_insert_with(|| match &cron.job {
            Some(job) => Node {
                identity: exact.clone(),
                name: safe_name(&job.name),
                purpose: job.description.as_deref().map(safe_purpose),
                state: Some(if job.enabled { "enabled" } else { "disabled" }.into()),
                availability: Availability::Available,
                schedule: Some(safe_purpose(&job.schedule)),
                stop: Stop {
                    allowed: false,
                    reason: Some("graph stop applies only to Blueprints".into()),
                },
                ..Node::default()
            },
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
            node.state = Some(format!("{:?}", run.status).to_lowercase());
            node.state_source = Some("cron_run".into());
            worker_statuses.insert(worker_id.clone(), run.started_at);
        }
        let edge_id = format!("{}:{}:{}", exact.id, run.run_id, tab_id);
        if !edges.iter().any(|edge| edge.id == edge_id) {
            edges.push(Edge {
                id: edge_id,
                source: exact.clone(),
                target: worker_id,
                bucket: "run".into(),
                handler_id: run.run_id.clone(),
                handler_type: "cron".into(),
                result: Some(HandlerResult {
                    status: Some(format!("{:?}", run.status).to_lowercase()),
                    execution_id: Some(run.run_id.clone()),
                    accepted_at: Some(run.started_at),
                    command_result: None,
                }),
                evidence: EdgeEvidence::Observed {
                    execution_id: run.run_id.clone(),
                },
            });
        }
    }
    *truncated |= cron.runs.len() > MAX_RUNS_PER_CRON;
}

pub fn project(
    workspace: &str,
    team: Option<&str>,
    jobs: &[Sentinel],
    mux: &dyn MuxView,
    crons: &BTreeMap<String, ConnectedCron>,
    configured_fallback: Option<codeterm_sentinel::graph_contracts::ConfiguredFallback>,
    now_ms: u64,
    live_leases: &BTreeSet<String>,
) -> Result<Snapshot, String> {
    validate_view_scope(mux, workspace, team)?;
    let mut scenarios: Vec<_> = jobs
        .iter()
        .filter(|j| {
            is_snapshot_root(j)
                && j.workspace_id == workspace
                && j.team_id.as_deref() == team
                && has_live_owner(j, mux)
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
                let (label, timeout_ms) = match &handler.action {
                    Route::Command(parameters) => {
                        (Some(parameters.label.clone()), Some(parameters.timeout_ms))
                    }
                    _ => (None, None),
                };
                handlers.push(GraphHandler {
                    id: handler.id.clone(),
                    r#type: super::routes::action_name(&handler.action).into(),
                    target: target.as_ref().map(|target| target.0.clone()),
                    availability: if target.as_ref().is_none_or(|target| target.1) {
                        Availability::Available
                    } else {
                        Availability::Missing
                    },
                    last_result: result.clone(),
                    label,
                    timeout_ms,
                });
                if let Some((target, available)) = target {
                    if edges.len() >= MAX_EDGES {
                        truncated = true;
                        continue;
                    }
                    edges.push(Edge {
                        id: format!("{}:{}:{}", job.id, bucket, handler.id),
                        source: exact.clone(),
                        target: target.clone(),
                        bucket: bucket.clone(),
                        handler_id: handler.id.clone(),
                        handler_type: super::routes::action_name(&handler.action).into(),
                        result,
                        evidence: EdgeEvidence::Configured {
                            route_id: handler.id.clone(),
                        },
                    });
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
            outcomes.push(Outcome {
                bucket: bucket.clone(),
                handlers,
            });
        }
        let last_result = last.map(|transition| LastResult {
            bucket: transition.bucket.clone(),
            accepted_at: transition.accepted_at,
            handlers: transition
                .handlers
                .iter()
                .map(|handler| TransitionHandler {
                    handler_id: handler.handler_id.clone(),
                    result: record(handler),
                })
                .collect(),
            decision: detail::decision_projection(transition.decision.as_ref()),
        });
        let history = history::project(job, &mut history_budget);
        let history_truncated = history.len() < job.transitions.len();
        let solution_success = job.transitions.iter().any(|transition| {
            transition.bucket != "other"
                && (job.until.as_ref() == Some(&transition.bucket)
                    || transition
                        .handlers
                        .iter()
                        .any(|handler| handler.status == super::handlers::Status::Completed))
        });
        let state = if job.machine.stopped && executing {
            "finishing"
        } else if job.machine.stopped {
            "stopped"
        } else if expired {
            "expired"
        } else if job.admission.held_reason.is_some()
            || (held_reason(job).is_some() && !job.machine.paused)
            || job.machine.held.is_some()
        {
            "held"
        } else if job.machine.paused {
            "paused"
        } else {
            "active"
        };
        let usage = super::session::usage(jobs, job, now_ms);
        nodes.insert(
            exact.clone(),
            Node {
                identity: exact.clone(),
                name: safe_name(job.name.as_deref().unwrap_or(&job.id)),
                purpose: Some(safe_purpose(&job.question)),
                state: Some(state.into()),
                held_reason: Some(held_reason(job).map(str::to_owned)),
                availability: Availability::Available,
                hidden: Some(false),
                cadence_ms: Some(job.interval_ms),
                conversation: Some(Conversation {
                    enabled: job.session,
                    exchanges: usage.exchanges,
                    actions: usage.actions,
                    safety_limit:
                        codeterm_blueprint_types::blueprint_terminal::SESSION_SAFETY_LIMIT,
                    rate_limit_per_minute:
                        codeterm_blueprint_types::blueprint_terminal::SESSION_RATE_LIMIT,
                }),
                expires_at: job.expires_at,
                stopped_at: Some(job.cleanup.stopped_at),
                deletes_at: Some(job.cleanup.deletes_at),
                note: Some(job.note.clone()),
                solution_success: Some(solution_success),
                outcomes,
                last_result,
                history: Some(history),
                history_truncated: Some(history_truncated),
                stop: Stop {
                    allowed: executing || (!job.machine.stopped && !expired),
                    reason: if executing {
                        None
                    } else if job.machine.stopped {
                        job.last_reason.as_deref().map(safe_purpose)
                    } else if expired {
                        Some("deadline elapsed".into())
                    } else {
                        None
                    },
                },
                detail: Some(detail::project(
                    job, mux, workspace, team, now_ms, executing,
                )),
                source: Some(source_descriptor(job)),
                blueprint_definition: Some(AgentBlueprintDefinition {
                    source: "stored_agent_definition".into(),
                    definition_id: job.id.clone(),
                    creator_tab_id: job.creator_tab_id.clone(),
                    definition_version: job.definition_version,
                    lineage: job
                        .amendments
                        .iter()
                        .map(|amendment| DefinitionLineage {
                            version: amendment.version,
                            replaced_version: amendment.replaced_version,
                            author_tab_id: amendment.author_tab_id.clone(),
                            cause_transition_id: amendment.cause_transition_id.clone(),
                            at: amendment.at,
                        })
                        .collect(),
                }),
                ..Node::default()
            },
        );
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
        quarantined: Vec::new(),
    })
}
