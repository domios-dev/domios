//! Lossless mapping between domain Sentinels and typed Storage records.

use std::collections::{BTreeMap, BTreeSet};

use crate::sentinel::amend::{Amendment, AmendmentLineage, DefinitionSnapshot, DefinitionVersion};
use crate::sentinel::command::{Capture, Parameters, ResultMetadata, Status as CommandStatus};
use crate::sentinel::graph::Identity;
use crate::sentinel::handlers::{Handler, ResultRecord, Status, Transition};
use crate::sentinel::machine::{EscalateTarget, Machine, Route, SafeKind};
use crate::sentinel::source::{RunBinding, TabBinding};
use crate::sentinel::store::Sentinel;
use crate::storage::{
    AmendmentRecord, BatchFence, BucketDefinition, CaptureRecord, CommandParameters,
    CommandResultRecord, CommandStatus as StoredCommandStatus, DefinitionProvenance,
    DefinitionUpdate, EscalateTarget as StoredEscalateTarget, HandlerAction, HandlerDefinition,
    HandlerResultRecord, HandlerStatus, NewTransition, ObserveKind, ObserveSource, RunRecord,
    RuntimeOutcome, SafeKind as StoredSafeKind, SentinelCreate, SentinelId, SentinelIdentity,
    SentinelImport, SentinelScope, StageDefinition, StageRuntime, StageSource, StoredDefinition,
    StoredSentinel, TargetIdentity, TransitionRecord,
};

pub(super) fn id(id: &str) -> Result<SentinelId, String> {
    SentinelId::new(id).map_err(|error| format!("invalid blueprint identity: {error}"))
}

fn safe_kind(kind: &SafeKind) -> StoredSafeKind {
    match kind {
        SafeKind::DirectoryTrust => StoredSafeKind::DirectoryTrust,
        SafeKind::Onboarding => StoredSafeKind::Onboarding,
        SafeKind::Update => StoredSafeKind::Update,
        SafeKind::Permission => StoredSafeKind::Permission,
        SafeKind::Structured => StoredSafeKind::Structured,
        SafeKind::Other(kind) => StoredSafeKind::Other(kind.clone()),
    }
}

fn domain_safe_kind(kind: StoredSafeKind) -> SafeKind {
    match kind {
        StoredSafeKind::DirectoryTrust => SafeKind::DirectoryTrust,
        StoredSafeKind::Onboarding => SafeKind::Onboarding,
        StoredSafeKind::Update => SafeKind::Update,
        StoredSafeKind::Permission => SafeKind::Permission,
        StoredSafeKind::Structured => SafeKind::Structured,
        StoredSafeKind::Other(kind) => SafeKind::Other(kind),
    }
}

fn action(route: &Route) -> HandlerAction {
    match route {
        Route::Notify => HandlerAction::Notify,
        Route::Stop => HandlerAction::Stop,
        Route::Send(target) => HandlerAction::Send(target.clone()),
        Route::Cron(target) => HandlerAction::Cron(target.clone()),
        Route::Sentinel(target) => HandlerAction::Sentinel(target.clone()),
        Route::Command(parameters) => HandlerAction::Command(CommandParameters {
            command: parameters.command.clone(),
            label: parameters.label.clone(),
            timeout_ms: parameters.timeout_ms,
        }),
        Route::RespondPrompt(kind) => HandlerAction::RespondPrompt(safe_kind(kind)),
        Route::Escalate(EscalateTarget::Creator) => {
            HandlerAction::Escalate(StoredEscalateTarget::Creator)
        }
        Route::Escalate(EscalateTarget::Agent(tab)) => {
            HandlerAction::Escalate(StoredEscalateTarget::Agent(tab.clone()))
        }
    }
}

fn route(action: HandlerAction) -> Route {
    match action {
        HandlerAction::Notify => Route::Notify,
        HandlerAction::Stop => Route::Stop,
        HandlerAction::Send(target) => Route::Send(target),
        HandlerAction::Cron(target) => Route::Cron(target),
        HandlerAction::Sentinel(target) => Route::Sentinel(target),
        HandlerAction::Command(parameters) => Route::Command(Parameters {
            command: parameters.command,
            label: parameters.label,
            timeout_ms: parameters.timeout_ms,
        }),
        HandlerAction::RespondPrompt(kind) => Route::RespondPrompt(domain_safe_kind(kind)),
        HandlerAction::Escalate(StoredEscalateTarget::Creator) => {
            Route::Escalate(EscalateTarget::Creator)
        }
        HandlerAction::Escalate(StoredEscalateTarget::Agent(tab)) => {
            Route::Escalate(EscalateTarget::Agent(tab))
        }
    }
}

fn handler_definition(handler: &Handler) -> HandlerDefinition {
    HandlerDefinition {
        id: handler.id.clone(),
        action: action(&handler.action),
        target: handler.target.as_ref().map(target),
    }
}

fn domain_handler(handler: HandlerDefinition) -> Handler {
    Handler {
        id: handler.id,
        action: route(handler.action),
        target: handler.target.map(identity),
    }
}

fn observe(binding: &TabBinding) -> ObserveSource {
    let (kind, reference) = match binding {
        TabBinding::Tab(id) => (ObserveKind::Tab, id),
        TabBinding::CronRun(id) => (ObserveKind::CronRun, id),
    };
    ObserveSource {
        kind,
        reference: reference.clone(),
    }
}

fn domain_observe(source: ObserveSource) -> TabBinding {
    match source.kind {
        ObserveKind::Tab => TabBinding::Tab(source.reference),
        ObserveKind::CronRun => TabBinding::CronRun(source.reference),
    }
}

fn amendment(amendment: &Amendment) -> AmendmentRecord {
    AmendmentRecord {
        version: amendment.version,
        replaced_version: amendment.replaced_version,
        author_tab_id: amendment.author_tab_id.clone(),
        cause_transition_id: amendment.cause_transition_id.clone(),
        authored_at: amendment.at,
    }
}

fn domain_amendment(record: AmendmentRecord) -> Amendment {
    Amendment {
        version: record.version,
        replaced_version: record.replaced_version,
        author_tab_id: record.author_tab_id,
        cause_transition_id: record.cause_transition_id,
        at: record.authored_at,
        previous_definition: None,
    }
}

fn target(identity: &Identity) -> TargetIdentity {
    TargetIdentity {
        kind: identity.kind.clone(),
        id: identity.id.clone(),
        workspace_id: identity.workspace_id.clone(),
        team_id: identity.team_id.clone(),
        owner_tab_id: identity.owner_tab_id.clone(),
    }
}

fn identity(target: TargetIdentity) -> Identity {
    Identity {
        kind: target.kind,
        id: target.id,
        workspace_id: target.workspace_id,
        team_id: target.team_id,
        owner_tab_id: target.owner_tab_id,
    }
}

fn status(status: &Status) -> HandlerStatus {
    match status {
        Status::Unconfirmed => HandlerStatus::Unconfirmed,
        Status::Completed => HandlerStatus::Completed,
        Status::Failed => HandlerStatus::Failed,
        Status::Noop => HandlerStatus::Noop,
        Status::Skipped => HandlerStatus::Skipped,
    }
}

fn domain_status(status: HandlerStatus) -> Status {
    match status {
        HandlerStatus::Unconfirmed => Status::Unconfirmed,
        HandlerStatus::Completed => Status::Completed,
        HandlerStatus::Failed => Status::Failed,
        HandlerStatus::Noop => Status::Noop,
        HandlerStatus::Skipped => Status::Skipped,
    }
}

fn command_status(status: CommandStatus) -> StoredCommandStatus {
    match status {
        CommandStatus::Success => StoredCommandStatus::Success,
        CommandStatus::Nonzero => StoredCommandStatus::Nonzero,
        CommandStatus::Error => StoredCommandStatus::Error,
        CommandStatus::Cancelled => StoredCommandStatus::Cancelled,
        CommandStatus::Deadline => StoredCommandStatus::Deadline,
        CommandStatus::Unconfirmed => StoredCommandStatus::Unconfirmed,
    }
}

fn domain_command_status(status: StoredCommandStatus) -> CommandStatus {
    match status {
        StoredCommandStatus::Success => CommandStatus::Success,
        StoredCommandStatus::Nonzero => CommandStatus::Nonzero,
        StoredCommandStatus::Error => CommandStatus::Error,
        StoredCommandStatus::Cancelled => CommandStatus::Cancelled,
        StoredCommandStatus::Deadline => CommandStatus::Deadline,
        StoredCommandStatus::Unconfirmed => CommandStatus::Unconfirmed,
    }
}

fn capture(capture: &Capture) -> CaptureRecord {
    CaptureRecord {
        total_bytes: capture.total_bytes,
        retained_bytes: capture.retained_bytes,
        truncated: capture.truncated,
    }
}

fn domain_capture(capture: CaptureRecord) -> Capture {
    Capture {
        total_bytes: capture.total_bytes,
        retained_bytes: capture.retained_bytes,
        truncated: capture.truncated,
    }
}

fn metadata(metadata: &ResultMetadata) -> CommandResultRecord {
    CommandResultRecord {
        status: command_status(metadata.status),
        exit_code: metadata.exit_code,
        resources_confirmed: metadata.resources_confirmed,
        stdout: metadata.stdout.as_ref().map(capture),
        stderr: metadata.stderr.as_ref().map(capture),
        started_at: metadata.started_at,
        finished_at: metadata.finished_at,
        primary: metadata.primary.clone(),
        cleanup: metadata.cleanup.clone(),
    }
}

fn domain_metadata(record: CommandResultRecord) -> ResultMetadata {
    ResultMetadata {
        status: domain_command_status(record.status),
        exit_code: record.exit_code,
        resources_confirmed: record.resources_confirmed,
        stdout: record.stdout.map(domain_capture),
        stderr: record.stderr.map(domain_capture),
        started_at: record.started_at,
        finished_at: record.finished_at,
        primary: record.primary,
        cleanup: record.cleanup,
    }
}

pub(super) fn result(result: &ResultRecord) -> HandlerResultRecord {
    HandlerResultRecord {
        handler_id: result.handler_id.clone(),
        status: status(&result.status),
        claim_id: result.claim_id.clone(),
        execution_id: result.execution_id.clone(),
        accepted_at: result.accepted_at,
        command_result: result.command_result.as_ref().map(metadata),
        reason: result.reason.clone(),
    }
}

fn domain_result(record: HandlerResultRecord) -> ResultRecord {
    ResultRecord {
        handler_id: record.handler_id,
        status: domain_status(record.status),
        claim_id: record.claim_id,
        execution_id: record.execution_id,
        accepted_at: record.accepted_at,
        command_result: record.command_result.map(domain_metadata),
        reason: record.reason,
    }
}

/// Transitions recorded before versioned history keep `definition_version: None`.
fn transition(transition: &Transition) -> TransitionRecord {
    TransitionRecord {
        causal_run_json: transition
            .causal_run
            .as_ref()
            .map(|run| serde_json::json!(run).to_string()),
        decision_json: transition
            .decision
            .as_ref()
            .and_then(|decision| serde_json::to_string(decision).ok()),
        id: transition.id.clone(),
        bucket: transition.bucket.clone(),
        accepted_at: transition.accepted_at,
        terminal: transition.terminal,
        definition_version: None,
        handlers: transition.handlers.iter().map(result).collect(),
    }
}

pub(super) fn new_transition(transition: &Transition) -> NewTransition {
    NewTransition {
        causal_run_json: transition
            .causal_run
            .as_ref()
            .map(|run| serde_json::json!(run).to_string()),
        decision_json: transition
            .decision
            .as_ref()
            .and_then(|decision| serde_json::to_string(decision).ok()),
        id: transition.id.clone(),
        bucket: transition.bucket.clone(),
        accepted_at: transition.accepted_at,
        terminal: transition.terminal,
    }
}

fn domain_transition(record: TransitionRecord) -> Transition {
    Transition {
        causal_run: record
            .causal_run_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok()),
        decision: record
            .decision_json
            .as_deref()
            .and_then(|json| {
                serde_json::from_str::<crate::sentinel::provenance::Decision>(json).ok()
            })
            .filter(|decision| decision.valid()),
        id: record.id,
        bucket: record.bucket,
        accepted_at: record.accepted_at,
        terminal: record.terminal,
        handlers: record.handlers.into_iter().map(domain_result).collect(),
    }
}

/// Refuses any definition whose source, buckets, routes and matches would not round-trip.
fn definition(job: &Sentinel) -> Result<StageDefinition, String> {
    let declared: BTreeSet<&String> = job.buckets.iter().collect();
    let exact = declared.len() == job.buckets.len()
        && job.routes.len() == job.buckets.len()
        && job.routes.keys().all(|bucket| declared.contains(bucket))
        && job.matches.keys().all(|bucket| declared.contains(bucket));
    if !exact {
        return Err("blueprint definition is not losslessly representable".into());
    }
    use super::super::stage_source::StageSource as DomainSource;
    let source = match &job.source {
        DomainSource::Probe(probe) => StageSource::Probe(probe.clone()),
        DomainSource::Observe(binding) => StageSource::Observe(observe(binding)),
        DomainSource::Input(input) => StageSource::Input(input.clone()),
        DomainSource::Terminal { binding, config } => StageSource::Terminal {
            binding: observe(binding),
            config: config.clone(),
        },
        DomainSource::Pipeline(spec) => StageSource::Pipeline(spec.clone()),
    };
    let buckets = job
        .buckets
        .iter()
        .map(|bucket| BucketDefinition {
            name: bucket.clone(),
            match_pattern: job.matches.get(bucket).cloned(),
            rearm_on_change: job.rearm_on_change.contains(bucket),
            handlers: job.routes[bucket].iter().map(handler_definition).collect(),
        })
        .collect();
    Ok(StageDefinition {
        session: job.session,
        name: job.name.clone(),
        source,
        question: job.question.clone(),
        interval_ms: job.interval_ms,
        until: job.until.clone(),
        escalation_timeout_ms: job.escalation_timeout_ms,
        max_transitions: job.max_transitions,
        max_actions: job.max_actions,
        buckets,
        on_other: job.on_other.iter().map(handler_definition).collect(),
    })
}

pub(super) fn update_definition(
    job: &Sentinel,
    author_tab_id: String,
    cause_transition_id: Option<String>,
    authored_at: u64,
) -> Result<DefinitionUpdate, String> {
    Ok(DefinitionUpdate {
        definition: definition(job)?,
        author_tab_id,
        cause_transition_id,
        authored_at,
    })
}

pub(super) fn history_version(stored: StoredDefinition) -> DefinitionVersion {
    let definition = stored.definition;
    let terminal = definition.terminal().cloned();
    let pipeline = definition.pipeline().cloned();
    let (probe, observe, input) = match definition.source {
        StageSource::Probe(command) => (command, None, None),
        StageSource::Observe(source) => (String::new(), Some(domain_observe(source)), None),
        StageSource::Terminal { binding, .. } => {
            (String::new(), Some(domain_observe(binding)), None)
        }
        StageSource::Input(input) => (String::new(), None, Some(input)),
        StageSource::Pipeline(_) => (String::new(), None, None),
    };
    let mut buckets = Vec::with_capacity(definition.buckets.len());
    let mut routes = BTreeMap::new();
    let mut matches = BTreeMap::new();
    let mut rearm_on_change = BTreeSet::new();
    for bucket in definition.buckets {
        if let Some(pattern) = bucket.match_pattern {
            matches.insert(bucket.name.clone(), pattern);
        }
        if bucket.rearm_on_change {
            rearm_on_change.insert(bucket.name.clone());
        }
        routes.insert(
            bucket.name.clone(),
            bucket.handlers.into_iter().map(domain_handler).collect(),
        );
        buckets.push(bucket.name);
    }
    let amendment = stored.amendment.map(|amendment| AmendmentLineage {
        replaced_version: amendment.replaced_version,
        author_tab_id: amendment.author_tab_id,
        cause_transition_id: amendment.cause_transition_id,
        authored_at: amendment.authored_at,
    });
    DefinitionVersion {
        version: stored.version,
        provenance: match stored.provenance {
            DefinitionProvenance::Authored => "authored",
            DefinitionProvenance::Imported => "imported",
        }
        .into(),
        definition: DefinitionSnapshot {
            terminal,
            session: definition.session,
            name: definition.name,
            pipeline,
            probe,
            observe,
            input,
            question: definition.question,
            interval_ms: definition.interval_ms,
            buckets,
            routes,
            matches,
            rearm_on_change,
            until: definition.until,
            max_transitions: definition.max_transitions,
            max_actions: definition.max_actions,
            on_other: definition
                .on_other
                .into_iter()
                .map(domain_handler)
                .collect(),
            escalation_timeout_ms: definition.escalation_timeout_ms,
        },
        amendment,
    }
}

fn runtime(job: &Sentinel) -> StageRuntime {
    StageRuntime {
        causal_run_json: job
            .causal_run
            .as_ref()
            .map(|run| serde_json::json!(run).to_string()),
        generation: job.revision,
        next_run: job.next_run,
        expires_at: job.expires_at,
        paused: job.machine.paused,
        stopped: job.machine.stopped,
        last_hash: job.machine.last_hash.clone(),
        last_bucket: job.machine.last_bucket.clone(),
        last_reason: job.last_reason.clone(),
        held: job.machine.held.clone(),
        run: job.run.as_ref().map(|run| RunRecord {
            run_id: run.run_id.clone(),
            tab_id: run.tab_id.clone(),
            bound_at: run.bound_at,
        }),
        run_transitions: job.run_transitions,
    }
}

/// The imported head's own lineage; earlier amendments have no stored definition to join.
fn head_amendment(job: &Sentinel) -> Result<Option<AmendmentRecord>, String> {
    match job.amendments.as_slice() {
        [] if job.definition_version == 1 => Ok(None),
        [head] if head.version == job.definition_version => Ok(Some(amendment(head))),
        _ => Err("blueprint amendment lineage is not losslessly representable".into()),
    }
}

fn stage_identity(job: &Sentinel) -> Result<SentinelIdentity, String> {
    Ok(SentinelIdentity {
        id: id(&job.id)?,
        scope: SentinelScope {
            creator_tab_id: job.creator_tab_id.clone(),
            workspace_id: job.workspace_id.clone(),
            team_id: job.team_id.clone(),
        },
        cwd: job.cwd.clone(),
        agent_authored: job.agent_authored,
    })
}

fn scenario(job: &Sentinel) -> Result<Option<SentinelId>, String> {
    job.scenario_id.as_deref().map(id).transpose()
}

pub(super) fn create(job: &Sentinel, associate: &[String]) -> Result<SentinelCreate, String> {
    if !job.transitions.is_empty() {
        return Err("a new blueprint carries no accepted transitions".into());
    }
    if job.definition_version != 1 || !job.amendments.is_empty() {
        return Err("a new blueprint starts at definition version 1 without amendments".into());
    }
    Ok(SentinelCreate {
        identity: stage_identity(job)?,
        scenario_id: scenario(job)?,
        definition: definition(job)?,
        runtime: runtime(job),
        associate: associate
            .iter()
            .map(|child| id(child))
            .collect::<Result<_, _>>()?,
    })
}

/// Exact existing state for the immutable import path; nothing here can start work.
pub(super) fn import(job: &Sentinel) -> Result<SentinelImport, String> {
    Ok(SentinelImport {
        identity: stage_identity(job)?,
        scenario_id: scenario(job)?,
        version: job.definition_version,
        amendment: head_amendment(job)?,
        definition: definition(job)?,
        runtime: runtime(job),
        transitions: job.transitions.iter().map(transition).collect(),
    })
}

pub(super) fn outcome(job: &Sentinel) -> RuntimeOutcome {
    RuntimeOutcome {
        last_hash: job.machine.last_hash.clone(),
        last_bucket: job.machine.last_bucket.clone(),
        stopped: job.machine.stopped,
        last_reason: job.last_reason.clone(),
        held: job.machine.held.clone(),
        run_transitions: job.run_transitions,
    }
}

pub(super) fn fence(job: &Sentinel, transition: &str) -> BatchFence {
    BatchFence {
        generation: job.revision,
        last_hash: job.machine.last_hash.clone(),
        last_bucket: job.machine.last_bucket.clone(),
        transition_id: transition.to_string(),
    }
}

/// The committed aggregate as a domain Sentinel; the definition is the current head version.
pub(super) fn decode(stored: &StoredSentinel) -> Sentinel {
    let record = stored.record.clone();
    let definition_version = record.definition.version;
    let definition = record.definition.definition;
    let terminal = definition.terminal().cloned();
    let pipeline = definition.pipeline().cloned();
    let (probe, observe, input) = match definition.source {
        StageSource::Probe(command) => (command, None, None),
        StageSource::Observe(source) => (String::new(), Some(domain_observe(source)), None),
        StageSource::Terminal { binding, .. } => {
            (String::new(), Some(domain_observe(binding)), None)
        }
        StageSource::Input(input) => (String::new(), None, Some(input)),
        StageSource::Pipeline(_) => (String::new(), None, None),
    };
    let mut buckets = Vec::with_capacity(definition.buckets.len());
    let mut routes = BTreeMap::new();
    let mut matches = BTreeMap::new();
    let mut rearm_on_change = BTreeSet::new();
    for bucket in definition.buckets {
        if let Some(pattern) = bucket.match_pattern {
            matches.insert(bucket.name.clone(), pattern);
        }
        if bucket.rearm_on_change {
            rearm_on_change.insert(bucket.name.clone());
        }
        let handlers: Vec<Handler> = bucket.handlers.into_iter().map(domain_handler).collect();
        routes.insert(bucket.name.clone(), handlers);
        buckets.push(bucket.name);
    }
    let runtime = record.runtime;
    Sentinel {
        source: super::super::stage_source::StageSource::from_parts(
            probe, observe, input, terminal, pipeline,
        )
        .expect("stored source was validated"),
        session: definition.session,
        causal_run: runtime
            .causal_run_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok()),
        id: record.identity.id.as_str().to_string(),
        team_id: record.identity.scope.team_id,
        creator_tab_id: record.identity.scope.creator_tab_id,
        workspace_id: record.identity.scope.workspace_id,
        cwd: record.identity.cwd,
        question: definition.question,
        buckets,
        interval_ms: definition.interval_ms,
        routes,
        matches,
        rearm_on_change,
        until: definition.until,
        expires_at: runtime.expires_at,
        next_run: runtime.next_run,
        machine: Machine {
            last_hash: runtime.last_hash,
            last_bucket: runtime.last_bucket,
            stopped: runtime.stopped,
            paused: runtime.paused,
            held: runtime.held,
        },
        last_reason: runtime.last_reason,
        run_transitions: runtime.run_transitions,
        revision: runtime.generation,
        agent_authored: record.identity.agent_authored,
        name: definition.name,
        scenario_id: record.scenario_id.map(|id| id.as_str().to_string()),
        max_transitions: definition.max_transitions,
        max_actions: definition.max_actions,
        transitions: record
            .transitions
            .into_iter()
            .map(domain_transition)
            .collect(),
        run: runtime.run.map(|run| RunBinding {
            run_id: run.run_id,
            tab_id: run.tab_id,
            bound_at: run.bound_at,
        }),
        on_other: definition
            .on_other
            .into_iter()
            .map(domain_handler)
            .collect(),
        escalation_timeout_ms: definition.escalation_timeout_ms,
        definition_version,
        amendments: record
            .amendments
            .into_iter()
            .map(domain_amendment)
            .collect(),
    }
}
