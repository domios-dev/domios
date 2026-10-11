mod journal;
pub use journal::{BlueprintJournalEntry, BlueprintJournalWrite};
mod source;
mod transition;
pub use source::{ObserveKind, ObserveSource, SourceKind, StageSource};

use crate::error::StorageError;
use crate::repositories::{to_sql_u64, Revision};
use std::collections::HashSet;

/// Repository bounds protect the mailbox and reads; domain rules stay in the domain.
pub const MAX_SENTINEL_RECORDS: usize = 1024;
pub const MAX_SENTINEL_BATCH: usize = 1024;
/// Newest amended versions carried by one aggregate; `history` still returns every version.
pub const MAX_SENTINEL_LINEAGE: usize = 16;
/// Reserved bucket holding the handlers that run when no declared bucket matches.
pub const OTHER_BUCKET: &str = "other";
const MAX_BUCKETS: usize = 64;
const MAX_HANDLERS_PER_BUCKET: usize = 64;
pub(super) const MAX_TRANSITIONS: usize = 65_536;
const MAX_RESULTS: usize = 64;
const MAX_CLEANUP: usize = 16;
const MAX_ID_BYTES: usize = 256;
const MAX_TEXT_BYTES: usize = 32 * 1024;
const RECORD_OVERHEAD: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SentinelId(String);

impl SentinelId {
    pub fn new(id: impl Into<String>) -> Result<Self, StorageError> {
        let id = id.into();
        identifier("blueprint id", &id)?;
        Ok(SentinelId(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(super) fn trusted(id: String) -> Self {
        SentinelId(id)
    }
}

/// Exact authority scope; a `None` Team is its own scope, never another Team's.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SentinelScope {
    pub creator_tab_id: String,
    pub workspace_id: String,
    pub team_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentinelIdentity {
    pub id: SentinelId,
    pub scope: SentinelScope,
    pub cwd: String,
    pub agent_authored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandParameters {
    pub command: String,
    pub label: String,
    pub timeout_ms: u64,
}

/// Prompt class a respond-prompt handler may request an answer for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafeKind {
    DirectoryTrust,
    Onboarding,
    Update,
    Permission,
    Structured,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscalateTarget {
    Creator,
    Agent(String),
}

/// Stored command definitions are data only; the repository never executes anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerAction {
    Notify,
    Send(String),
    Cron(String),
    Sentinel(String),
    Stop,
    Command(CommandParameters),
    RespondPrompt(SafeKind),
    Escalate(EscalateTarget),
}

/// Exact historical target identity; it never implies a live tab, cron or Team exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetIdentity {
    pub kind: String,
    pub id: String,
    pub workspace_id: String,
    pub team_id: Option<String>,
    pub owner_tab_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerDefinition {
    pub id: String,
    pub action: HandlerAction,
    pub target: Option<TargetIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BucketDefinition {
    pub name: String,
    pub match_pattern: Option<String>,
    pub rearm_on_change: bool,
    pub handlers: Vec<HandlerDefinition>,
}

/// Authored configuration of one stage; runtime state never lives here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageDefinition {
    pub session: bool,
    pub name: Option<String>,
    pub source: StageSource,
    pub question: String,
    pub interval_ms: u64,
    pub until: Option<String>,
    pub escalation_timeout_ms: Option<u64>,
    pub max_transitions: u16,
    pub max_actions: u16,
    pub buckets: Vec<BucketDefinition>,
    /// Handlers of the reserved `other` bucket.
    pub on_other: Vec<HandlerDefinition>,
}

/// `Imported` versions were observed at import; any earlier history is unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionProvenance {
    Authored,
    Imported,
}

/// Lineage of one amended version: which head it replaced, who authored it and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmendmentRecord {
    pub version: u64,
    pub replaced_version: u64,
    pub author_tab_id: String,
    pub cause_transition_id: Option<String>,
    pub authored_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDefinition {
    pub version: u64,
    pub provenance: DefinitionProvenance,
    pub created_revision: Revision,
    pub amendment: Option<AmendmentRecord>,
    pub definition: StageDefinition,
}

/// One authored replacement of the head version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionUpdate {
    pub definition: StageDefinition,
    pub author_tab_id: String,
    pub cause_transition_id: Option<String>,
    pub authored_at: u64,
}

/// The cron run whose tab a stage currently observes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub run_id: String,
    pub tab_id: String,
    pub bound_at: u64,
}

/// Mutable stage state. `generation` is the domain lease fence, distinct from the
/// definition version and from the Storage commit revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageRuntime {
    pub runs_since_transition: u64,
    pub last_probe_at: Option<u64>,
    pub probe_state: Option<String>,
    pub causal_run_json: Option<String>,
    pub generation: u64,
    pub next_run: u64,
    pub expires_at: Option<u64>,
    pub paused: bool,
    pub stopped: bool,
    pub last_hash: Option<String>,
    pub last_bucket: Option<String>,
    pub last_reason: Option<String>,
    /// The `other` transition whose escalation holds the stage.
    pub held: Option<String>,
    pub run: Option<RunRecord>,
    pub run_transitions: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerStatus {
    Unconfirmed,
    Completed,
    Failed,
    Noop,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    Success,
    Nonzero,
    Error,
    Cancelled,
    Deadline,
    Unconfirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureRecord {
    pub total_bytes: u64,
    pub retained_bytes: u64,
    pub truncated: bool,
}

/// Command completion metadata only; raw command output is never stored here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResultRecord {
    pub status: CommandStatus,
    pub exit_code: Option<i32>,
    pub resources_confirmed: bool,
    pub stdout: Option<CaptureRecord>,
    pub stderr: Option<CaptureRecord>,
    pub started_at: u64,
    pub finished_at: u64,
    pub primary: Option<String>,
    pub cleanup: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerResultRecord {
    pub handler_id: String,
    pub status: HandlerStatus,
    pub claim_id: Option<String>,
    pub execution_id: Option<String>,
    pub accepted_at: Option<u64>,
    pub command_result: Option<CommandResultRecord>,
    pub reason: Option<String>,
}

/// `definition_version` is `None` for transitions accepted before versioned history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionRecord {
    pub causal_run_json: Option<String>,
    pub decision_json: Option<String>,
    pub id: String,
    pub bucket: String,
    pub accepted_at: u64,
    pub terminal: bool,
    pub definition_version: Option<u64>,
    pub handlers: Vec<HandlerResultRecord>,
}

/// One coherent committed aggregate: current definition version plus runtime and results.
/// `amendments` are the newest amended versions up to the head, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentinelRecord {
    pub identity: SentinelIdentity,
    pub scenario_id: Option<SentinelId>,
    pub definition: StoredDefinition,
    pub amendments: Vec<AmendmentRecord>,
    pub runtime: StageRuntime,
    pub transitions: Vec<TransitionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSentinel {
    pub record: SentinelRecord,
    pub revision: Revision,
    pub ordinal: u64,
}

/// A new stage; `associate` names unassociated stages that join its scenario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentinelCreate {
    pub identity: SentinelIdentity,
    pub scenario_id: Option<SentinelId>,
    pub definition: StageDefinition,
    pub runtime: StageRuntime,
    pub associate: Vec<SentinelId>,
}

/// Exact existing state from an immutable interchange source; `version` is the observed head
/// and `amendment` its own lineage, since earlier definitions are unavailable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentinelImport {
    pub identity: SentinelIdentity,
    pub scenario_id: Option<SentinelId>,
    pub version: u64,
    pub amendment: Option<AmendmentRecord>,
    pub definition: StageDefinition,
    pub runtime: StageRuntime,
    pub transitions: Vec<TransitionRecord>,
}

/// The persisted part of a lease's batch liveness that every action re-asserts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchFence {
    pub generation: u64,
    pub last_hash: Option<String>,
    pub last_bucket: Option<String>,
    pub transition_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeOutcome {
    pub runs_since_transition: u64,
    pub last_probe_at: Option<u64>,
    pub probe_state: Option<String>,
    pub last_hash: Option<String>,
    pub last_bucket: Option<String>,
    pub stopped: bool,
    pub last_reason: Option<String>,
    pub held: Option<String>,
    pub run_transitions: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTransition {
    pub causal_run_json: Option<String>,
    pub decision_json: Option<String>,
    pub id: String,
    pub bucket: String,
    pub accepted_at: u64,
    pub terminal: bool,
}

/// Operational scheduling changes; never a new authored definition version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleUpdate {
    Advance { id: SentinelId, next_run: u64 },
    Stop { id: SentinelId, reason: String },
}

/// Why a fenced operation wrote nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fence {
    Missing,
    Duplicate,
    Capacity,
    Generation,
    Stopped,
    Paused,
    Observation,
    Transition,
    Budget,
    Undeclared,
    Claimed,
    NotClaimed,
    Scope,
    Version,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationOutcome {
    Applied,
    Unchanged,
    Fenced(Fence),
}

/// Post-commit aggregates of every stage the operation touched, for cache publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentinelCommit {
    pub outcome: MutationOutcome,
    pub sentinels: Vec<StoredSentinel>,
    pub removed: Vec<SentinelId>,
}

impl SentinelCommit {
    pub(super) fn fenced(fence: Fence, current: Option<StoredSentinel>) -> Self {
        SentinelCommit {
            outcome: MutationOutcome::Fenced(fence),
            sentinels: current.into_iter().collect(),
            removed: Vec::new(),
        }
    }
}

pub(super) fn identifier(field: &'static str, value: &str) -> Result<(), StorageError> {
    if value.is_empty() || value.len() > MAX_ID_BYTES || value.chars().any(char::is_control) {
        return Err(StorageError::invalid(
            field,
            "must be 1..=256 bytes without control characters",
        ));
    }
    Ok(())
}

fn optional_identifier(field: &'static str, value: &Option<String>) -> Result<(), StorageError> {
    value
        .as_deref()
        .map_or(Ok(()), |value| identifier(field, value))
}

pub(super) fn text(field: &'static str, value: &str) -> Result<(), StorageError> {
    if value.len() > MAX_TEXT_BYTES {
        return Err(StorageError::invalid(
            field,
            format!("exceeds {MAX_TEXT_BYTES} bytes"),
        ));
    }
    Ok(())
}

fn optional_text(field: &'static str, value: &Option<String>) -> Result<(), StorageError> {
    value.as_deref().map_or(Ok(()), |value| text(field, value))
}

fn integer(field: &'static str, value: u64) -> Result<(), StorageError> {
    to_sql_u64(field, value).map(|_| ())
}

fn count(field: &'static str, len: usize, limit: usize) -> Result<(), StorageError> {
    if len > limit {
        return Err(StorageError::invalid(
            field,
            format!("exceeds {limit} entries"),
        ));
    }
    Ok(())
}

fn optional_len(value: &Option<String>) -> usize {
    value.as_ref().map_or(0, String::len)
}

impl SentinelIdentity {
    pub(super) fn validate(&self) -> Result<(), StorageError> {
        identifier("creator tab id", &self.scope.creator_tab_id)?;
        identifier("workspace id", &self.scope.workspace_id)?;
        optional_identifier("team id", &self.scope.team_id)?;
        if self.cwd.is_empty() {
            return Err(StorageError::invalid("blueprint cwd", "must be nonempty"));
        }
        text("blueprint cwd", &self.cwd)
    }

    fn cost(&self) -> usize {
        self.id.0.len()
            + self.scope.creator_tab_id.len()
            + self.scope.workspace_id.len()
            + optional_len(&self.scope.team_id)
            + self.cwd.len()
    }
}

impl TargetIdentity {
    fn validate(&self) -> Result<(), StorageError> {
        identifier("target kind", &self.kind)?;
        identifier("target id", &self.id)?;
        text("target workspace", &self.workspace_id)?;
        optional_identifier("target team", &self.team_id)?;
        optional_identifier("target owner", &self.owner_tab_id)
    }

    fn cost(&self) -> usize {
        self.kind.len()
            + self.id.len()
            + self.workspace_id.len()
            + optional_len(&self.team_id)
            + optional_len(&self.owner_tab_id)
    }
}

impl HandlerDefinition {
    fn validate(&self) -> Result<(), StorageError> {
        identifier("handler id", &self.id)?;
        match &self.action {
            HandlerAction::Send(target)
            | HandlerAction::Cron(target)
            | HandlerAction::Sentinel(target) => identifier("handler target", target)?,
            HandlerAction::Command(parameters) => {
                text("command", &parameters.command)?;
                text("command label", &parameters.label)?;
                integer("command timeout", parameters.timeout_ms)?;
            }
            HandlerAction::Escalate(EscalateTarget::Agent(tab)) => {
                identifier("escalation target", tab)?
            }
            HandlerAction::RespondPrompt(SafeKind::Other(kind)) => {
                if kind.trim().is_empty() {
                    return Err(StorageError::invalid(
                        "safe prompt kind",
                        "must be nonempty",
                    ));
                }
                identifier("safe prompt kind", kind)?
            }
            HandlerAction::Notify
            | HandlerAction::Stop
            | HandlerAction::RespondPrompt(_)
            | HandlerAction::Escalate(EscalateTarget::Creator) => {}
        }
        self.target
            .as_ref()
            .map_or(Ok(()), TargetIdentity::validate)
    }

    fn cost(&self) -> usize {
        let action = match &self.action {
            HandlerAction::Send(target)
            | HandlerAction::Cron(target)
            | HandlerAction::Sentinel(target) => target.len(),
            HandlerAction::Command(parameters) => parameters.command.len() + parameters.label.len(),
            HandlerAction::Escalate(EscalateTarget::Agent(tab)) => tab.len(),
            HandlerAction::Notify
            | HandlerAction::Stop
            | HandlerAction::RespondPrompt(_)
            | HandlerAction::Escalate(EscalateTarget::Creator) => 0,
        };
        RECORD_OVERHEAD
            + self.id.len()
            + action
            + self.target.as_ref().map_or(0, TargetIdentity::cost)
    }
}

impl StageDefinition {
    pub fn terminal(&self) -> Option<&codeterm_types::blueprint_terminal::Config> {
        if let StageSource::Terminal { config, .. } = &self.source {
            Some(config)
        } else {
            None
        }
    }
    pub fn pipeline(&self) -> Option<&codeterm_types::blueprint_pipeline::BlueprintSpec> {
        if let StageSource::Pipeline(spec) = &self.source {
            Some(spec)
        } else {
            None
        }
    }

    pub(super) fn validate(&self) -> Result<(), StorageError> {
        if let Some(terminal) = self.terminal() {
            terminal
                .validate()
                .map_err(|error| StorageError::invalid("terminal", error))?;
            if !matches!(&self.source, StageSource::Terminal { binding, .. } if binding.kind == ObserveKind::Tab)
            {
                return Err(StorageError::invalid(
                    "terminal",
                    "requires a direct tab observation",
                ));
            }
        }
        optional_text("blueprint name", &self.name)?;
        match &self.source {
            StageSource::Probe(command) => text("blueprint probe", command)?,
            StageSource::Observe(observe) => identifier("observe source", &observe.reference)?,
            StageSource::Terminal { binding, .. } => {
                identifier("terminal source", &binding.reference)?
            }
            StageSource::Pipeline(_) => {}
            StageSource::Input(input) => codeterm_types::sentinel_input::validate(input)
                .map_err(|error| StorageError::invalid("blueprint input", error.to_string()))?,
        }
        if let Some(pipeline) = self.pipeline() {
            let bytes = serde_json::to_vec(pipeline)
                .map_err(|_| StorageError::invalid("pipeline", "cannot be encoded"))?;
            if bytes.len() > 65_536 {
                return Err(StorageError::invalid(
                    "pipeline",
                    "must be bounded and use the pipeline scheduler source",
                ));
            }
        }
        text("blueprint question", &self.question)?;
        integer("blueprint interval", self.interval_ms)?;
        if let Some(timeout) = self.escalation_timeout_ms {
            integer("escalation timeout", timeout)?;
        }
        if self.interval_ms == 0
            || self.max_transitions == 0
            || self.max_actions == 0
            || self.escalation_timeout_ms == Some(0)
        {
            return Err(StorageError::invalid(
                "blueprint definition",
                "interval, escalation timeout and budgets must be positive",
            ));
        }
        count("blueprint buckets", self.buckets.len(), MAX_BUCKETS)?;
        count(
            "other handlers",
            self.on_other.len(),
            MAX_HANDLERS_PER_BUCKET,
        )?;
        let mut buckets = HashSet::new();
        let mut handlers = HashSet::new();
        for handler in &self.on_other {
            handler.validate()?;
            if !handlers.insert(handler.id.as_str()) {
                return Err(StorageError::invalid(
                    "blueprint handlers",
                    "repeat a handler id",
                ));
            }
        }
        for bucket in &self.buckets {
            identifier("bucket", &bucket.name)?;
            if bucket.name == OTHER_BUCKET {
                return Err(StorageError::invalid("bucket", "other is reserved"));
            }
            optional_text("bucket match", &bucket.match_pattern)?;
            count(
                "bucket handlers",
                bucket.handlers.len(),
                MAX_HANDLERS_PER_BUCKET,
            )?;
            if !buckets.insert(bucket.name.as_str()) {
                return Err(StorageError::invalid(
                    "blueprint buckets",
                    "repeat a bucket",
                ));
            }
            for handler in &bucket.handlers {
                handler.validate()?;
                if !handlers.insert(handler.id.as_str()) {
                    return Err(StorageError::invalid(
                        "blueprint handlers",
                        "repeat a handler id",
                    ));
                }
            }
        }
        if let Some(until) = &self.until {
            if !buckets.contains(until.as_str()) {
                return Err(StorageError::invalid("blueprint until", "names no bucket"));
            }
        }
        Ok(())
    }

    pub(super) fn cost(&self) -> usize {
        let buckets: usize = self
            .buckets
            .iter()
            .map(|bucket| {
                RECORD_OVERHEAD
                    + bucket.name.len()
                    + optional_len(&bucket.match_pattern)
                    + bucket
                        .handlers
                        .iter()
                        .map(HandlerDefinition::cost)
                        .sum::<usize>()
            })
            .sum();
        RECORD_OVERHEAD
            + optional_len(&self.name)
            + match &self.source {
                StageSource::Probe(command) => command.len(),
                StageSource::Observe(observe) => observe.reference.len(),
                StageSource::Terminal { binding, config } => {
                    binding.reference.len()
                        + serde_json::to_vec(config).map_or(0, |bytes| bytes.len())
                }
                StageSource::Pipeline(spec) => {
                    serde_json::to_vec(spec).map_or(0, |bytes| bytes.len())
                }
                StageSource::Input(input) => {
                    serde_json::to_vec(input).map_or(0, |bytes| bytes.len())
                }
            }
            + self.question.len()
            + optional_len(&self.until)
            + buckets
            + self
                .on_other
                .iter()
                .map(HandlerDefinition::cost)
                .sum::<usize>()
    }
}

impl AmendmentRecord {
    pub(super) fn validate(&self) -> Result<(), StorageError> {
        integer("definition version", self.version)?;
        integer("replaced version", self.replaced_version)?;
        identifier("amendment author", &self.author_tab_id)?;
        optional_identifier("amendment cause", &self.cause_transition_id)?;
        integer("amendment time", self.authored_at)?;
        if self.replaced_version == 0 || self.replaced_version >= self.version {
            return Err(StorageError::invalid(
                "amendment",
                "must replace an earlier version",
            ));
        }
        Ok(())
    }

    fn cost(&self) -> usize {
        RECORD_OVERHEAD + self.author_tab_id.len() + optional_len(&self.cause_transition_id)
    }
}

impl DefinitionUpdate {
    pub(super) fn validate(&self) -> Result<(), StorageError> {
        self.definition.validate()?;
        identifier("amendment author", &self.author_tab_id)?;
        optional_identifier("amendment cause", &self.cause_transition_id)?;
        integer("amendment time", self.authored_at)
    }

    pub(super) fn cost(&self) -> usize {
        self.definition.cost() + self.author_tab_id.len() + optional_len(&self.cause_transition_id)
    }
}

impl StageRuntime {
    pub(super) fn validate(&self) -> Result<(), StorageError> {
        integer("probe count", self.runs_since_transition)?;
        if let Some(at) = self.last_probe_at {
            integer("last probe", at)?;
        }
        optional_text("probe state", &self.probe_state)?;
        optional_text("causal run", &self.causal_run_json)?;
        integer("runtime generation", self.generation)?;
        integer("runtime next run", self.next_run)?;
        if let Some(expires_at) = self.expires_at {
            integer("runtime expiry", expires_at)?;
        }
        optional_text("runtime hash", &self.last_hash)?;
        optional_text("runtime bucket", &self.last_bucket)?;
        optional_text("runtime reason", &self.last_reason)?;
        optional_identifier("held transition", &self.held)?;
        if self.run_transitions > 256 {
            return Err(StorageError::invalid(
                "run transition count",
                "exceeds the per-run transition limit",
            ));
        }
        if let Some(run) = &self.run {
            identifier("run id", &run.run_id)?;
            identifier("run tab", &run.tab_id)?;
            integer("run bound time", run.bound_at)?;
        }
        Ok(())
    }

    fn cost(&self) -> usize {
        RECORD_OVERHEAD
            + optional_len(&self.causal_run_json)
            + optional_len(&self.last_hash)
            + optional_len(&self.last_bucket)
            + optional_len(&self.last_reason)
            + optional_len(&self.probe_state)
            + optional_len(&self.held)
            + self
                .run
                .as_ref()
                .map_or(0, |run| run.run_id.len() + run.tab_id.len())
    }
}

impl CommandResultRecord {
    fn validate(&self) -> Result<(), StorageError> {
        integer("command started", self.started_at)?;
        integer("command finished", self.finished_at)?;
        for capture in [&self.stdout, &self.stderr].into_iter().flatten() {
            integer("capture total", capture.total_bytes)?;
            integer("capture retained", capture.retained_bytes)?;
        }
        optional_text("command primary failure", &self.primary)?;
        count("command cleanup", self.cleanup.len(), MAX_CLEANUP)?;
        self.cleanup
            .iter()
            .try_for_each(|error| text("command cleanup", error))
    }

    fn cost(&self) -> usize {
        RECORD_OVERHEAD
            + optional_len(&self.primary)
            + self.cleanup.iter().map(String::len).sum::<usize>()
    }
}

impl HandlerResultRecord {
    pub(super) fn validate(&self) -> Result<(), StorageError> {
        identifier("result handler id", &self.handler_id)?;
        optional_identifier("claim id", &self.claim_id)?;
        optional_identifier("execution id", &self.execution_id)?;
        if let Some(accepted_at) = self.accepted_at {
            integer("claim time", accepted_at)?;
        }
        optional_text("result reason", &self.reason)?;
        let skipped = self.status == HandlerStatus::Skipped;
        if self.claim_id.is_none() != self.accepted_at.is_none()
            || skipped != self.claim_id.is_none()
            || (skipped && self.execution_id.is_some())
        {
            return Err(StorageError::invalid(
                "handler result",
                "claim identity does not match its status",
            ));
        }
        self.command_result
            .as_ref()
            .map_or(Ok(()), CommandResultRecord::validate)
    }

    pub(super) fn cost(&self) -> usize {
        RECORD_OVERHEAD
            + self.handler_id.len()
            + optional_len(&self.claim_id)
            + optional_len(&self.execution_id)
            + optional_len(&self.reason)
            + self
                .command_result
                .as_ref()
                .map_or(0, CommandResultRecord::cost)
    }
}

impl RuntimeOutcome {
    pub(super) fn validate(&self) -> Result<(), StorageError> {
        integer("probe count", self.runs_since_transition)?;
        if let Some(at) = self.last_probe_at {
            integer("last probe", at)?;
        }
        optional_text("probe state", &self.probe_state)?;
        optional_text("runtime hash", &self.last_hash)?;
        optional_text("runtime bucket", &self.last_bucket)?;
        optional_text("runtime reason", &self.last_reason)?;
        optional_identifier("held transition", &self.held)?;
        if self.run_transitions > 256 {
            return Err(StorageError::invalid(
                "run transition count",
                "exceeds the per-run transition limit",
            ));
        }
        Ok(())
    }

    pub(super) fn cost(&self) -> usize {
        RECORD_OVERHEAD
            + optional_len(&self.last_hash)
            + optional_len(&self.last_bucket)
            + optional_len(&self.last_reason)
            + optional_len(&self.probe_state)
            + optional_len(&self.held)
    }
}

impl BatchFence {
    pub(super) fn validate(&self) -> Result<(), StorageError> {
        integer("fence generation", self.generation)?;
        optional_text("fence hash", &self.last_hash)?;
        optional_text("fence bucket", &self.last_bucket)?;
        identifier("fence transition", &self.transition_id)
    }

    pub(super) fn cost(&self) -> usize {
        RECORD_OVERHEAD
            + optional_len(&self.last_hash)
            + optional_len(&self.last_bucket)
            + self.transition_id.len()
    }
}

impl SentinelCreate {
    pub(super) fn validate(&self) -> Result<(), StorageError> {
        self.identity.validate()?;
        self.definition.validate()?;
        self.runtime.validate()?;
        validate_run_transition_count(&self.definition, &self.runtime)?;
        count(
            "scenario associations",
            self.associate.len(),
            MAX_SENTINEL_RECORDS,
        )
    }

    pub(super) fn cost(&self) -> usize {
        RECORD_OVERHEAD
            + self.identity.cost()
            + self.definition.cost()
            + self.runtime.cost()
            + self
                .associate
                .iter()
                .map(|id| id.0.len() + RECORD_OVERHEAD)
                .sum::<usize>()
    }
}

impl SentinelImport {
    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        self.identity.validate()?;
        self.definition.validate()?;
        self.runtime.validate()?;
        validate_run_transition_count(&self.definition, &self.runtime)?;
        integer("definition version", self.version)?;
        match &self.amendment {
            Some(amendment) => {
                amendment.validate()?;
                if amendment.version != self.version {
                    return Err(StorageError::invalid(
                        "imported amendment",
                        "must describe the imported version",
                    ));
                }
            }
            None if self.version != 1 => {
                return Err(StorageError::invalid(
                    "imported definition",
                    "a later version requires its amendment lineage",
                ));
            }
            None => {}
        }
        count("transitions", self.transitions.len(), MAX_TRANSITIONS)?;
        let mut ids = HashSet::new();
        for transition in &self.transitions {
            transition.validate()?;
            if transition.definition_version.is_some() {
                return Err(StorageError::invalid(
                    "imported transition",
                    "predates versioned history and names no definition version",
                ));
            }
            if !ids.insert(transition.id.as_str()) {
                return Err(StorageError::invalid(
                    "transitions",
                    "repeat a transition id",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn cost(&self) -> usize {
        RECORD_OVERHEAD
            + self.identity.cost()
            + self.definition.cost()
            + self.runtime.cost()
            + self.amendment.as_ref().map_or(0, AmendmentRecord::cost)
            + self
                .transitions
                .iter()
                .map(TransitionRecord::cost)
                .sum::<usize>()
    }
}

fn validate_run_transition_count(
    definition: &StageDefinition,
    runtime: &StageRuntime,
) -> Result<(), StorageError> {
    let cron_run = matches!(
        &definition.source,
        StageSource::Observe(observe) if observe.kind == ObserveKind::CronRun
    );
    if (!definition.session && runtime.run_transitions > definition.max_transitions)
        || (runtime.run_transitions > 0 && (!cron_run || runtime.run.is_none()))
    {
        return Err(StorageError::invalid(
            "run transition count",
            "does not match the active CronRun allowance",
        ));
    }
    Ok(())
}

impl ScheduleUpdate {
    pub(super) fn validate(&self) -> Result<(), StorageError> {
        match self {
            ScheduleUpdate::Advance { next_run, .. } => integer("next run", *next_run),
            ScheduleUpdate::Stop { reason, .. } => text("stop reason", reason),
        }
    }

    pub(super) fn cost(&self) -> usize {
        RECORD_OVERHEAD
            + match self {
                ScheduleUpdate::Advance { id, .. } => id.0.len(),
                ScheduleUpdate::Stop { id, reason } => id.0.len() + reason.len(),
            }
    }
}

pub(super) fn overhead() -> usize {
    RECORD_OVERHEAD
}
