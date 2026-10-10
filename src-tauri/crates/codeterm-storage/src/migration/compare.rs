//! Typed semantic projections of imported domains and their differences: a gate inside the
//! import transaction before commit, and a report over committed reads for the receipt.

use super::manifest::{bounded, ExpectedDivergence, ImportRefusal};
use super::{DomainImport, ImportDomain, ImportPlan};
use crate::error::StorageError;
use crate::owner::Storage;
use crate::repositories::cron::{
    CronHistoryImport, CronImport, CronJobId, CronJobImport, CronLastRunDivergence, CronRunId,
    CronSnapshot,
};
use crate::repositories::desk::{DeskLayoutNode, DeskSnapshot};
use crate::repositories::sentinel::{DefinitionProvenance, SentinelImport, SentinelRecord};
use crate::repositories::Revision;
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

const MAX_MISMATCHES: usize = 64;

/// One differing fact, named by entity identity and field; values are never copied here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionMismatch {
    pub subject: String,
    pub field: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainComparison {
    pub domain: ImportDomain,
    /// Cron: live definitions; Sentinel: stages.
    pub expected_entities: u64,
    pub committed_entities: u64,
    /// Cron: listed history runs; Sentinel: accepted transitions.
    pub expected_items: u64,
    pub committed_items: u64,
    pub mismatches: Vec<ProjectionMismatch>,
    pub elided: u64,
}

/// Committed state read back through the public repositories after the import committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonReport {
    pub revision: Revision,
    pub domains: Vec<DomainComparison>,
}

impl ComparisonReport {
    pub fn is_clean(&self) -> bool {
        self.domains.iter().all(|domain| {
            domain.mismatches.is_empty()
                && domain.elided == 0
                && domain.expected_entities == domain.committed_entities
                && domain.expected_items == domain.committed_items
        })
    }
}

/// A bounded collector of mismatches for one domain.
pub(crate) struct Diff {
    domain: ImportDomain,
    mismatches: Vec<ProjectionMismatch>,
    elided: u64,
}

impl Diff {
    pub(crate) fn new(domain: ImportDomain) -> Self {
        Diff {
            domain,
            mismatches: Vec::new(),
            elided: 0,
        }
    }

    pub(crate) fn note(&mut self, subject: &str, field: &'static str) {
        if self.mismatches.len() < MAX_MISMATCHES {
            self.mismatches.push(ProjectionMismatch {
                subject: bounded(subject),
                field,
            });
        } else {
            self.elided += 1;
        }
    }

    /// Inside the transaction any difference refuses the commit.
    pub(crate) fn gate(self) -> Result<(), ImportRefusal> {
        if self.mismatches.is_empty() && self.elided == 0 {
            return Ok(());
        }
        Err(ImportRefusal::ProjectionMismatch {
            domain: self.domain,
            mismatches: self.mismatches,
            elided: self.elided,
        })
    }

    fn report(self, expected: (u64, u64), committed: (u64, u64)) -> DomainComparison {
        DomainComparison {
            domain: self.domain,
            expected_entities: expected.0,
            committed_entities: committed.0,
            expected_items: expected.1,
            committed_items: committed.1,
            mismatches: self.mismatches,
            elided: self.elided,
        }
    }
}

/// A divergent last-run copy as stored, with its committed provenance.
pub(crate) struct ObservedDivergence {
    pub(crate) job_id: CronJobId,
    pub(crate) run_id: CronRunId,
    pub(crate) divergence: CronLastRunDivergence,
}

/// The semantic cron domain: an explicitly empty history list of a live job is the same fact
/// as no list, and history lists are ordered by identity.
pub(crate) fn normalized_cron(import: &CronImport) -> CronImport {
    let live: HashSet<&CronJobId> = import.jobs.iter().map(|job| &job.record.id).collect();
    let mut histories: Vec<CronHistoryImport> = import
        .histories
        .iter()
        .filter(|history| !history.runs.is_empty() || !live.contains(&history.job_id))
        .cloned()
        .collect();
    histories.sort_by(|a, b| a.job_id.cmp(&b.job_id));
    CronImport {
        jobs: import.jobs.clone(),
        histories,
    }
}

fn cron_counts(import: &CronImport) -> (u64, u64) {
    let runs = import
        .histories
        .iter()
        .map(|history| history.runs.len() as u64)
        .sum();
    (import.jobs.len() as u64, runs)
}

pub(crate) fn diff_cron(expected: &CronImport, actual: &CronImport, diff: &mut Diff) {
    if expected.jobs.len() != actual.jobs.len() {
        diff.note("jobs", "count");
    }
    for (want, got) in expected.jobs.iter().zip(&actual.jobs) {
        let subject = want.record.id.as_str();
        if want.record.id != got.record.id {
            diff.note(subject, "order");
            continue;
        }
        if want.record != got.record {
            diff.note(subject, "definition");
        }
        if want.last_run != got.last_run {
            diff.note(subject, "last_run");
        }
    }
    let lists = |import: &CronImport| -> BTreeMap<CronJobId, CronHistoryImport> {
        import
            .histories
            .iter()
            .map(|history| (history.job_id.clone(), history.clone()))
            .collect()
    };
    let (want, got) = (lists(expected), lists(actual));
    if want.len() != expected.histories.len() || got.len() != actual.histories.len() {
        diff.note("histories", "repeated_identity");
    }
    for (id, history) in &want {
        match got.get(id) {
            None => diff.note(id.as_str(), "history_missing"),
            Some(other) if other.runs != history.runs => diff.note(id.as_str(), "history"),
            Some(_) => {}
        }
    }
    for id in got.keys().filter(|id| !want.contains_key(*id)) {
        diff.note(id.as_str(), "history_extra");
    }
}

pub(crate) fn diff_divergences(
    expected: &[ExpectedDivergence],
    observed: &[ObservedDivergence],
    revision: Revision,
    diff: &mut Diff,
) {
    let mut want: Vec<&ExpectedDivergence> = expected.iter().collect();
    want.sort_by(|a, b| a.job_id.cmp(&b.job_id));
    let mut got: Vec<&ObservedDivergence> = observed.iter().collect();
    got.sort_by(|a, b| a.job_id.cmp(&b.job_id));
    if want.len() != got.len() {
        diff.note("divergences", "count");
    }
    for (wanted, found) in want.iter().zip(&got) {
        let subject = wanted.job_id.as_str();
        if wanted.job_id != found.job_id || wanted.run_id != found.run_id {
            diff.note(subject, "divergence_identity");
            continue;
        }
        if found.divergence.origin != wanted.origin {
            diff.note(subject, "divergence_origin");
        }
        if found.divergence.imported_revision != revision {
            diff.note(subject, "divergence_revision");
        }
        if found.divergence.imported_differing != wanted.imported_differing {
            diff.note(subject, "divergence_fields");
        }
    }
}

pub(crate) fn diff_sentinel(
    expected: &[SentinelImport],
    actual: &[&SentinelRecord],
    diff: &mut Diff,
) {
    if expected.len() != actual.len() {
        diff.note("stages", "count");
    }
    for (want, got) in expected.iter().zip(actual) {
        let subject = want.identity.id.as_str();
        if want.identity != got.identity {
            diff.note(subject, "identity");
            continue;
        }
        if want.scenario_id != got.scenario_id {
            diff.note(subject, "scenario");
        }
        if got.definition.version != 1
            || got.definition.provenance != DefinitionProvenance::Imported
        {
            diff.note(subject, "definition_version");
        }
        if want.definition != got.definition.definition {
            diff.note(subject, "definition");
        }
        if want.runtime != got.runtime {
            diff.note(subject, "runtime");
        }
        if want.transitions != got.transitions {
            diff.note(subject, "transitions");
        }
    }
}

pub(crate) fn diff_desk(expected: &DeskSnapshot, actual: &DeskSnapshot, diff: &mut Diff) {
    if expected.version != actual.version {
        diff.note("desk", "version");
    }
    if expected.timestamp != actual.timestamp {
        diff.note("desk", "timestamp");
    }
    if expected.active_workspace_id != actual.active_workspace_id {
        diff.note("desk", "active_workspace");
    }
    if expected.workspaces != actual.workspaces {
        diff.note("workspaces", "content_or_order");
    }
    if expected.teams != actual.teams {
        diff.note("teams", "content_or_order");
    }
    if expected.agent_session_identities != actual.agent_session_identities {
        diff.note("identity_catalogue", "content_or_order");
    }
}

fn layout_count(node: &DeskLayoutNode) -> u64 {
    match node {
        DeskLayoutNode::Leaf { .. } => 1,
        DeskLayoutNode::Split { children, .. } => children
            .iter()
            .map(layout_count)
            .fold(1_u64, u64::saturating_add),
    }
}

fn desk_counts(snapshot: &DeskSnapshot) -> (u64, u64) {
    let tabs = snapshot
        .workspaces
        .iter()
        .map(|workspace| workspace.tabs.len() as u64)
        .fold(0_u64, u64::saturating_add);
    let entities = (snapshot.workspaces.len() as u64)
        .saturating_add(tabs)
        .saturating_add(snapshot.teams.len() as u64)
        .saturating_add(snapshot.agent_session_identities.len() as u64);
    let layouts = snapshot
        .workspaces
        .iter()
        .flat_map(|workspace| &workspace.tabs)
        .map(|tab| layout_count(&tab.layout))
        .fold(0_u64, u64::saturating_add);
    (entities, layouts)
}

fn transition_count(transitions: impl Iterator<Item = usize>) -> u64 {
    transitions.map(|count| count as u64).sum()
}

/// Projects the repository's own committed cron read into the import shape.
fn project_snapshot(snapshot: &CronSnapshot) -> (CronImport, Vec<ObservedDivergence>) {
    let mut jobs = Vec::new();
    let mut histories = Vec::new();
    let mut divergences = Vec::new();
    for state in &snapshot.states {
        if let Some(stored) = &state.job {
            jobs.push(CronJobImport {
                record: stored.record.clone(),
                last_run: stored.last_run.clone(),
            });
            if let (Some(divergence), Some(run)) = (&stored.divergence, &stored.last_run) {
                divergences.push(ObservedDivergence {
                    job_id: state.job_id.clone(),
                    run_id: run.run_id.clone(),
                    divergence: divergence.clone(),
                });
            }
        }
        if !state.history.is_empty() || state.job.is_none() {
            histories.push(CronHistoryImport {
                job_id: state.job_id.clone(),
                runs: state.history.clone(),
            });
        }
    }
    histories.sort_by(|a, b| a.job_id.cmp(&b.job_id));
    (CronImport { jobs, histories }, divergences)
}

/// Reads every imported domain back through its public repository after commit.
pub(crate) fn verify_committed(
    storage: &Storage,
    plan: &ImportPlan,
    revision: Revision,
    deadline: Duration,
) -> Result<ComparisonReport, StorageError> {
    let mut domains = Vec::new();
    for import in plan.domains() {
        domains.push(match import {
            DomainImport::Desk(expected) => {
                let state = storage.desk().snapshot().wait(deadline)?.value;
                let actual = &state.snapshot;
                let mut diff = Diff::new(ImportDomain::Desk);
                diff_desk(expected, actual, &mut diff);
                diff.report(desk_counts(expected), desk_counts(actual))
            }
            DomainImport::Cron(expected) => {
                let snapshot = storage.crons().snapshot().wait(deadline)?.value;
                let (actual, observed) = project_snapshot(&snapshot);
                let expected = normalized_cron(expected);
                let mut diff = Diff::new(ImportDomain::Cron);
                diff_cron(&expected, &actual, &mut diff);
                diff_divergences(plan.divergences(), &observed, revision, &mut diff);
                diff.report(cron_counts(&expected), cron_counts(&actual))
            }
            DomainImport::Sentinel(expected) => {
                let inventory = storage.sentinels().inventory().wait(deadline)?.value;
                let actual: Vec<&SentinelRecord> =
                    inventory.iter().map(|stored| &stored.record).collect();
                let mut diff = Diff::new(ImportDomain::Sentinel);
                diff_sentinel(expected, &actual, &mut diff);
                let expected_items =
                    transition_count(expected.iter().map(|record| record.transitions.len()));
                let committed_items =
                    transition_count(actual.iter().map(|record| record.transitions.len()));
                diff.report(
                    (expected.len() as u64, expected_items),
                    (actual.len() as u64, committed_items),
                )
            }
        });
    }
    Ok(ComparisonReport { revision, domains })
}
