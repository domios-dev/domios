use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};

#[cfg(test)]
use super::handlers::Definition;
use super::handlers::Handler;
use super::machine::Route;
use super::source::{bounded_id, TabBinding};
use super::storage::Staged;
use super::store::{Sentinel, Store};
use codeterm_blueprint_types::sentinel_input::InputSpec;

pub const MAX_AMENDMENTS: usize = codeterm_storage::MAX_SENTINEL_LINEAGE;
const MAX_UPDATE_BYTES: usize = 65_536;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinitionSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<serde_json::Value>,
    #[serde(default)]
    pub terminal: Option<codeterm_blueprint_types::blueprint_terminal::Config>,
    #[serde(default)]
    pub session: bool,
    #[serde(default)]
    pub note: Option<String>,
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<codeterm_blueprint_types::blueprint_pipeline::BlueprintSpec>,
    pub probe: String,
    pub observe: Option<TabBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<InputSpec>,
    pub question: String,
    pub interval_ms: u64,
    pub buckets: Vec<String>,
    pub routes: BTreeMap<String, Vec<Handler>>,
    pub matches: BTreeMap<String, String>,
    #[serde(default)]
    pub model_classification: bool,
    #[serde(default)]
    pub rearm_on_change: BTreeSet<String>,
    pub until: Option<String>,
    pub max_transitions: u16,
    pub max_actions: u16,
    pub on_other: Vec<Handler>,
    pub escalation_timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Amendment {
    pub version: u64,
    pub replaced_version: u64,
    pub author_tab_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause_transition_id: Option<String>,
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_definition: Option<Box<DefinitionSnapshot>>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DefinitionVersion {
    pub version: u64,
    pub provenance: String,
    pub definition: DefinitionSnapshot,
    pub amendment: Option<AmendmentLineage>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AmendmentLineage {
    pub replaced_version: u64,
    pub author_tab_id: String,
    pub cause_transition_id: Option<String>,
    pub authored_at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct History {
    #[serde(rename = "blueprint_id", alias = "sentinel_id")]
    pub sentinel_id: String,
    pub current_version: u64,
    pub history_truncated: bool,
    pub versions: Vec<DefinitionVersion>,
}

pub struct PreparedUpdate {
    pub job: Sentinel,
    pub cause_transition_id: Option<String>,
    pub note: Option<String>,
}

pub fn initial_version() -> u64 {
    1
}

pub fn is_initial_version(version: &u64) -> bool {
    *version == 1
}

pub fn validate(job: &Sentinel) -> Result<(), String> {
    let mut previous = 1;
    for amendment in &job.amendments {
        if amendment.version <= previous
            || amendment.version > job.definition_version
            || amendment.replaced_version == 0
            || amendment.replaced_version >= amendment.version
            || !bounded_id(&amendment.author_tab_id)
            || amendment
                .cause_transition_id
                .as_deref()
                .is_some_and(|cause| !bounded_id(cause))
            || amendment
                .previous_definition
                .as_ref()
                .is_some_and(|definition| {
                    definition
                        .rearm_on_change
                        .iter()
                        .any(|bucket| !definition.buckets.contains(bucket))
                        || serde_json::to_vec(definition)
                            .map_or(true, |bytes| bytes.len() > MAX_UPDATE_BYTES)
                })
        {
            return Err("invalid blueprint amendment lineage".into());
        }
        previous = amendment.version;
    }
    let head_lineage = job.amendments.last().map(|amendment| amendment.version);
    let expected = (job.definition_version > 1).then_some(job.definition_version);
    if job.definition_version == 0
        || job.amendments.len() > MAX_AMENDMENTS
        || head_lineage != expected
    {
        return Err("invalid blueprint definition version".into());
    }
    Ok(())
}

impl DefinitionSnapshot {
    pub fn from_job(job: &Sentinel) -> Self {
        Self {
            document: job.document.clone(),
            terminal: job.terminal().cloned(),
            session: job.session,
            name: job.name.clone(),
            note: job.note.clone(),
            pipeline: job.pipeline().cloned(),
            probe: job.probe().clone(),
            observe: job.observe().cloned(),
            input: job.input().cloned(),
            question: job.question.clone(),
            interval_ms: job.interval_ms,
            buckets: job.buckets.clone(),
            routes: job.routes.clone(),
            matches: job.matches.clone(),
            model_classification: job.model_classification,
            rearm_on_change: job.rearm_on_change.clone(),
            until: job.until.clone(),
            max_transitions: job.max_transitions,
            max_actions: job.max_actions,
            on_other: job.on_other.clone(),
            escalation_timeout_ms: job.escalation_timeout_ms,
        }
    }
}

impl Store {
    #[cfg(any(test, feature = "test-support"))]
    pub fn stage_update_definition(
        &mut self,
        expect_version: u64,
        author_tab_id: &str,
        authored_at: u64,
        update: PreparedUpdate,
    ) -> Staged<Sentinel> {
        self.stage_definition(
            expect_version,
            author_tab_id,
            authored_at,
            update,
            false,
            None,
        )
    }

    pub fn stage_apply_definition(
        &mut self,
        expect_version: u64,
        author_tab_id: &str,
        authored_at: u64,
        update: PreparedUpdate,
        stopped: Option<bool>,
    ) -> Staged<Sentinel> {
        self.stage_definition(
            expect_version,
            author_tab_id,
            authored_at,
            update,
            true,
            stopped,
        )
    }

    fn stage_definition(
        &mut self,
        expect_version: u64,
        author_tab_id: &str,
        authored_at: u64,
        update: PreparedUpdate,
        retarget: bool,
        stopped: Option<bool>,
    ) -> Staged<Sentinel> {
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        let Some(current) = self.get(&update.job.id).cloned() else {
            return Staged::ready(Err(format!("blueprint not found: {}", update.job.id)));
        };
        if current.definition_version != expect_version {
            return Staged::ready(Err(version_conflict(current.definition_version)));
        }
        if current.creator_tab_id != update.job.creator_tab_id
            || current.workspace_id != update.job.workspace_id
            || current.team_id != update.job.team_id
            || (!retarget
                && (current.observe().cloned() != update.job.observe().cloned()
                    || current.probe() != update.job.probe()
                    || current.input().map(|input| input.kind)
                        != update.job.input().map(|input| input.kind)))
        {
            return Staged::ready(Err(
                "blueprint identity and observation source are immutable".into(),
            ));
        }
        if self.jobs.iter().any(|job| {
            job.id != current.id
                && job.workspace_id == current.workspace_id
                && job.team_id == current.team_id
                && job.name == update.job.name
                && job.name.is_some()
        }) {
            return Staged::ready(Err(
                "Blueprint name already exists in this Team and workspace".into(),
            ));
        }
        let Some(next_version) = current.definition_version.checked_add(1) else {
            return Staged::ready(Err("blueprint definition version exhausted".into()));
        };
        let Some(next_revision) = current.revision.checked_add(1) else {
            return Staged::ready(Err("blueprint generation exhausted".into()));
        };
        let mut next = current.clone();
        if current.machine.stopped {
            next.machine = Default::default();
            next.cleanup.sync(false, authored_at);
            next.cleanup.transition_offset = current.transitions.len();
            next.cleanup.action_offset = current
                .transitions
                .iter()
                .flat_map(|t| &t.handlers)
                .filter(|h| h.claim_id.is_some())
                .count();
            next.progress = Default::default();
            next.run_transitions = 0;
            next.last_reason = None;
            next.expires_at = None;
            next.next_run = authored_at;
        }
        if let Some(stopped) = stopped {
            next.machine.stopped = stopped;
            next.cleanup.sync(stopped, authored_at);
        }
        next.document = update.job.document;
        next.session = update.job.session;
        next.name = update.job.name;
        next.note = update.job.note;
        next.source = update.job.source;
        next.question = update.job.question;
        next.buckets = update.job.buckets;
        next.interval_ms = update.job.interval_ms;
        next.routes = update.job.routes;
        next.matches = update.job.matches;
        next.model_classification = update.job.model_classification;
        next.rearm_on_change = update.job.rearm_on_change;
        next.until = update.job.until;
        next.max_transitions = update.job.max_transitions;
        next.max_actions = update.job.max_actions;
        next.on_other = update.job.on_other;
        next.escalation_timeout_ms = update.job.escalation_timeout_ms;
        if next
            .machine
            .last_bucket
            .as_ref()
            .is_some_and(|bucket| !next.buckets.contains(bucket))
        {
            next.machine.last_bucket = None;
        }
        next.definition_version = next_version;
        next.revision = next_revision;
        next.amendments.push(Amendment {
            version: next_version,
            replaced_version: current.definition_version,
            author_tab_id: author_tab_id.into(),
            cause_transition_id: update.cause_transition_id.clone(),
            at: authored_at,
            previous_definition: Some(Box::new(DefinitionSnapshot::from_job(&current))),
        });
        // Definition history holds earlier versions; the stage carries only its head lineage.
        let excess = next.amendments.len().saturating_sub(1);
        next.amendments.drain(..excess);
        if let Err(error) = self.association_ids(&next) {
            return Staged::ready(Err(error));
        }
        let mut jobs = self.jobs.clone();
        let Some(position) = jobs.iter().position(|job| job.id == current.id) else {
            return Staged::ready(Err(format!("blueprint not found: {}", current.id)));
        };
        jobs[position] = next.clone();
        let jobs = match super::store::validate(jobs) {
            Ok(jobs) => jobs,
            Err(error) => return Staged::ready(Err(error)),
        };
        if let Some(capability) = next.pending_capability() {
            return Staged::ready(Err(format!(
                "blueprint_capability_unavailable: {capability} is not executable yet"
            )));
        }
        if let Some(cancelled) = self.active.remove(&current.id) {
            cancelled.store(true, Ordering::SeqCst);
        }
        if !self.backend.is_json() {
            return self.sqlite_update_definition(
                expect_version,
                author_tab_id,
                authored_at,
                update.cause_transition_id,
                next,
            );
        }
        let previous = std::mem::replace(&mut self.jobs, jobs);
        match self.save_legacy() {
            Ok(()) => Staged::ready(Ok(next)),
            Err(error) => {
                self.jobs = previous;
                Staged::ready(Err(error))
            }
        }
    }

    pub fn typed_history(
        &self,
        id: &str,
    ) -> Result<
        Option<
            codeterm_storage::Completion<
                codeterm_storage::Read<Vec<codeterm_storage::StoredDefinition>>,
            >,
        >,
        String,
    > {
        let Some(job) = self.get(id) else {
            return Ok(None);
        };
        let Some(backend) = self.backend.sqlite() else {
            return Ok(None);
        };
        let id =
            codeterm_storage::SentinelId::new(job.id.clone()).map_err(|error| error.to_string())?;
        Ok(Some(backend.repo.history(id)))
    }
}

pub fn history(job: &Sentinel) -> History {
    let mut versions = Vec::new();
    for amendment in &job.amendments {
        let Some(definition) = amendment.previous_definition.as_deref() else {
            continue;
        };
        let lineage = job
            .amendments
            .iter()
            .find(|candidate| candidate.version == amendment.replaced_version)
            .map(lineage);
        versions.push(DefinitionVersion {
            version: amendment.replaced_version,
            provenance: "authored".into(),
            definition: definition.clone(),
            amendment: lineage,
        });
    }
    let head = job
        .amendments
        .last()
        .filter(|amendment| amendment.version == job.definition_version);
    versions.push(DefinitionVersion {
        version: job.definition_version,
        provenance: if head.is_some() {
            "authored"
        } else {
            "imported"
        }
        .into(),
        definition: DefinitionSnapshot::from_job(job),
        amendment: head.map(lineage),
    });
    versions.sort_by_key(|version| version.version);
    let history_truncated = job
        .amendments
        .first()
        .is_some_and(|amendment| amendment.replaced_version > 1)
        || job
            .amendments
            .iter()
            .any(|amendment| amendment.previous_definition.is_none());
    History {
        sentinel_id: job.id.clone(),
        current_version: job.definition_version,
        history_truncated,
        versions,
    }
}

pub fn from_storage_history(sentinel_id: String, versions: Vec<DefinitionVersion>) -> History {
    let current_version = versions.last().map_or(0, |version| version.version);
    History {
        sentinel_id,
        current_version,
        history_truncated: false,
        versions,
    }
}

pub fn version_conflict(current_version: u64) -> String {
    format!("blueprint_version_conflict: current_version={current_version}")
}

pub fn has_new_command(current: &Sentinel, next: &Sentinel) -> bool {
    let old: Vec<_> = current
        .routes
        .values()
        .flatten()
        .chain(&current.on_other)
        .filter_map(|handler| match &handler.action {
            Route::Command(parameters) => Some(parameters),
            _ => None,
        })
        .collect();
    next.routes
        .values()
        .flatten()
        .chain(&next.on_other)
        .filter_map(|handler| match &handler.action {
            Route::Command(parameters) => Some(parameters),
            _ => None,
        })
        .any(|parameters| !old.contains(&parameters))
}

fn lineage(amendment: &Amendment) -> AmendmentLineage {
    AmendmentLineage {
        replaced_version: amendment.replaced_version,
        author_tab_id: amendment.author_tab_id.clone(),
        cause_transition_id: amendment.cause_transition_id.clone(),
        authored_at: amendment.at,
    }
}

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
mod fixtures;
#[cfg(any(test, feature = "test-support"))]
pub use fixtures::{InputPatch, Update};
