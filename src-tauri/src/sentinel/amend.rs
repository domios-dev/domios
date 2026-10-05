use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};

use super::handlers::{self, Definition, Handler};
use super::machine::{self, Route};
use super::source::{bounded_id, TabBinding};
use super::storage::Staged;
use super::store::{Sentinel, Store};

pub const MAX_AMENDMENTS: usize = crate::storage::MAX_SENTINEL_LINEAGE;
const MAX_UPDATE_BYTES: usize = 65_536;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinitionSnapshot {
    pub name: Option<String>,
    pub probe: String,
    pub observe: Option<TabBinding>,
    pub question: String,
    pub interval_ms: u64,
    pub buckets: Vec<String>,
    pub routes: BTreeMap<String, Vec<Handler>>,
    pub matches: BTreeMap<String, String>,
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
    pub sentinel_id: String,
    pub current_version: u64,
    pub history_truncated: bool,
    pub versions: Vec<DefinitionVersion>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub name: Option<String>,
    pub question: String,
    pub buckets: Vec<String>,
    pub every: String,
    #[serde(default)]
    pub routes: BTreeMap<String, String>,
    #[serde(default)]
    pub matches: BTreeMap<String, String>,
    #[serde(default)]
    pub rearm_on_change: BTreeSet<String>,
    pub until: Option<String>,
    pub handlers: Option<BTreeMap<String, Vec<Definition>>>,
    #[serde(default)]
    pub on_other: Vec<Definition>,
    #[serde(default)]
    pub escalation_timeout_ms: Option<u64>,
    pub max_transitions: u16,
    pub max_actions: u16,
    #[serde(default)]
    pub cause: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

pub struct PreparedUpdate {
    pub job: Sentinel,
    pub cause_transition_id: Option<String>,
    pub note: Option<String>,
}

pub fn initial_version() -> u64 {
    1
}

pub(super) fn is_initial_version(version: &u64) -> bool {
    *version == 1
}

pub(super) fn validate(job: &Sentinel) -> Result<(), String> {
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
            return Err("invalid sentinel amendment lineage".into());
        }
        previous = amendment.version;
    }
    let head_lineage = job.amendments.last().map(|amendment| amendment.version);
    let expected = (job.definition_version > 1).then_some(job.definition_version);
    if job.definition_version == 0
        || job.amendments.len() > MAX_AMENDMENTS
        || head_lineage != expected
    {
        return Err("invalid sentinel definition version".into());
    }
    Ok(())
}

impl DefinitionSnapshot {
    pub fn from_job(job: &Sentinel) -> Self {
        Self {
            name: job.name.clone(),
            probe: job.probe.clone(),
            observe: job.observe.clone(),
            question: job.question.clone(),
            interval_ms: job.interval_ms,
            buckets: job.buckets.clone(),
            routes: job.routes.clone(),
            matches: job.matches.clone(),
            rearm_on_change: job.rearm_on_change.clone(),
            until: job.until.clone(),
            max_transitions: job.max_transitions,
            max_actions: job.max_actions,
            on_other: job.on_other.clone(),
            escalation_timeout_ms: job.escalation_timeout_ms,
        }
    }
}

impl Update {
    pub fn prepare(
        self,
        current: &Sentinel,
        author_tab_id: &str,
        connected: bool,
    ) -> Result<PreparedUpdate, String> {
        let name = self.name.as_deref().ok_or_else(|| {
            "Sentinel update definition needs a name; use a short name such as ci-green-watch".to_string()
        })?;
        codeterm_types::automation_name::validate_sentinel_name(name)?;
        if self.question.trim().is_empty() || self.question.len() > 4096 {
            return Err("question must be nonempty and bounded to 4096 bytes".into());
        }
        let interval_ms = super::store::duration_ms(&self.every)?;
        machine::validate(&self.buckets, &self.matches, connected, interval_ms)?;
        if self
            .rearm_on_change
            .iter()
            .any(|bucket| !self.buckets.contains(bucket))
        {
            return Err("rearm_on_change must name declared buckets".into());
        }
        if self
            .until
            .as_ref()
            .is_some_and(|bucket| !self.buckets.contains(bucket))
        {
            return Err("until must name a declared bucket".into());
        }
        let used_transitions = if matches!(
            current.observe.as_ref(),
            Some(super::source::TabBinding::CronRun(_))
        ) {
            usize::from(current.run_transitions)
        } else {
            current.transitions.len()
        };
        if self.max_transitions == 0
            || self.max_transitions > 256
            || usize::from(self.max_transitions) < used_transitions
            || self.max_actions == 0
            || self.max_actions > 1024
            || usize::from(self.max_actions) < claimed_actions(current)
        {
            return Err(
                "sentinel budgets must stay within limits and cannot shrink below use".into(),
            );
        }
        if self
            .cause
            .as_deref()
            .is_some_and(|cause| !bounded_id(cause))
        {
            return Err("cause must be a bounded transition ID".into());
        }
        if self.note.as_deref().is_some_and(|note| {
            note.trim().is_empty() || note.len() > 4096 || note.chars().any(char::is_control)
        }) {
            return Err("note must be bounded display text".into());
        }
        let routes = handlers::definitions(&self.buckets, self.routes, self.handlers)?;
        let mut other = BTreeMap::new();
        other.insert("other".into(), self.on_other);
        let on_other = handlers::definitions(&["other".into()], BTreeMap::new(), Some(other))?
            .remove("other")
            .unwrap_or_default();
        let mut job = current.clone();
        job.name = self.name;
        job.question = self.question;
        job.buckets = self.buckets;
        job.interval_ms = interval_ms;
        job.routes = routes;
        job.matches = self.matches;
        job.rearm_on_change = self.rearm_on_change;
        job.until = self.until;
        job.max_transitions = self.max_transitions;
        job.max_actions = self.max_actions;
        job.on_other = on_other;
        job.escalation_timeout_ms = self.escalation_timeout_ms;
        super::escalation::validate(&job)?;
        let snapshot = DefinitionSnapshot::from_job(&job);
        if serde_json::to_vec(&snapshot)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_UPDATE_BYTES
        {
            return Err("sentinel definition exceeds 64 KiB".into());
        }
        if author_tab_id != current.creator_tab_id && has_new_command(current, &job) {
            return Err("only the sentinel creator may introduce new command text".into());
        }
        Ok(PreparedUpdate {
            job,
            cause_transition_id: self.cause,
            note: self.note,
        })
    }
}

impl Store {
    pub(crate) fn stage_update_definition(
        &mut self,
        expect_version: u64,
        author_tab_id: &str,
        authored_at: u64,
        update: PreparedUpdate,
    ) -> Staged<Sentinel> {
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        let Some(current) = self.get(&update.job.id).cloned() else {
            return Staged::ready(Err(format!("sentinel not found: {}", update.job.id)));
        };
        if current.definition_version != expect_version {
            return Staged::ready(Err(version_conflict(current.definition_version)));
        }
        if current.machine.stopped
            || current
                .expires_at
                .is_some_and(|at| self.clock.now_ms() >= at)
        {
            return Staged::ready(Err("stopped sentinels cannot be amended".into()));
        }
        if current.creator_tab_id != update.job.creator_tab_id
            || current.workspace_id != update.job.workspace_id
            || current.team_id != update.job.team_id
            || current.observe != update.job.observe
            || current.probe != update.job.probe
        {
            return Staged::ready(Err(
                "sentinel identity and observation source are immutable".into(),
            ));
        }
        let Some(next_version) = current.definition_version.checked_add(1) else {
            return Staged::ready(Err("sentinel definition version exhausted".into()));
        };
        let Some(next_revision) = current.revision.checked_add(1) else {
            return Staged::ready(Err("sentinel generation exhausted".into()));
        };
        let mut next = current.clone();
        next.name = update.job.name;
        next.question = update.job.question;
        next.buckets = update.job.buckets;
        next.interval_ms = update.job.interval_ms;
        next.routes = update.job.routes;
        next.matches = update.job.matches;
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
        if next.amendments.len() > MAX_AMENDMENTS {
            next.amendments.remove(0);
        }
        if let Err(error) = self.association_ids(&next) {
            return Staged::ready(Err(error));
        }
        let mut jobs = self.jobs.clone();
        let Some(position) = jobs.iter().position(|job| job.id == current.id) else {
            return Staged::ready(Err(format!("sentinel not found: {}", current.id)));
        };
        jobs[position] = next.clone();
        let jobs = match super::store::validate(jobs) {
            Ok(jobs) => jobs,
            Err(error) => return Staged::ready(Err(error)),
        };
        if let Some(capability) = next.pending_capability() {
            return Staged::ready(Err(format!(
                "sentinel_capability_unavailable: {capability} is not executable yet"
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

    pub(crate) fn typed_history(
        &self,
        id: &str,
    ) -> Result<
        Option<
            crate::storage::Completion<crate::storage::Read<Vec<crate::storage::StoredDefinition>>>,
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
            crate::storage::SentinelId::new(job.id.clone()).map_err(|error| error.to_string())?;
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

pub(crate) fn version_conflict(current_version: u64) -> String {
    format!("sentinel_version_conflict: current_version={current_version}")
}

fn claimed_actions(job: &Sentinel) -> usize {
    job.transitions
        .iter()
        .flat_map(|transition| &transition.handlers)
        .filter(|handler| handler.claim_id.is_some())
        .count()
}

fn has_new_command(current: &Sentinel, next: &Sentinel) -> bool {
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
