//! Sentinel join: the repository bounds reachable from here, `runtime::import`, and a
//! comparison of the aggregates it read back inside the transaction with the original.

use super::compare::{self, Diff};
use super::manifest::{bounded, Facts, ImportRefusal};
use super::{DomainImport, Halt, ImportDomain};
use crate::error::{sqlite_error, StorageError};
use crate::owner::WriteCx;
use crate::repositories::sentinel::{
    runtime, HandlerAction, HandlerStatus, MutationOutcome, SentinelCommit, SentinelImport,
    SentinelRecord, MAX_SENTINEL_BATCH, MAX_SENTINEL_RECORDS,
};
use rusqlite::Connection;
use std::collections::HashSet;

const DOMAIN: ImportDomain = ImportDomain::Sentinel;

fn bound(name: &'static str, limit: usize, observed: usize) -> Result<(), ImportRefusal> {
    if observed > limit {
        return Err(ImportRefusal::Bound {
            domain: DOMAIN,
            bound: name,
            limit: limit as u64,
            observed: observed as u64,
        });
    }
    Ok(())
}

fn invalid(detail: String) -> ImportRefusal {
    ImportRefusal::Invalid {
        domain: DOMAIN,
        detail: bounded(&detail),
    }
}

/// The repository's own record validation (every field and count limit), then stage
/// count, batch, identity and scenario closure.
pub(super) fn preflight(
    records: &[SentinelImport],
    facts: &mut Facts,
) -> Result<(), ImportRefusal> {
    bound("stages", MAX_SENTINEL_RECORDS, records.len())?;
    bound("import-batch", MAX_SENTINEL_BATCH, records.len())?;
    let mut ids = HashSet::new();
    for record in records {
        record
            .validate()
            .map_err(|error| invalid(format!("stage {}: {error}", record.identity.id.as_str())))?;
        if !ids.insert(record.identity.id.as_str()) {
            return Err(invalid(format!(
                "repeats stage {}",
                record.identity.id.as_str()
            )));
        }
    }
    for record in records {
        let Some(scenario) = &record.scenario_id else {
            continue;
        };
        if !ids.contains(scenario.as_str()) {
            return Err(invalid(format!(
                "stage {} names scenario {} outside the import",
                record.identity.id.as_str(),
                scenario.as_str()
            )));
        }
    }
    let results = records
        .iter()
        .flat_map(|record| &record.transitions)
        .flat_map(|transition| &transition.handlers);
    let (handled, unconfirmed) = results.fold((0_u64, 0_u64), |(all, open), result| {
        let pending = u64::from(result.status == HandlerStatus::Unconfirmed);
        (all + 1, open + pending)
    });
    let transitions: u64 = records
        .iter()
        .map(|record| record.transitions.len() as u64)
        .sum();
    facts.bound(
        DOMAIN,
        "stages",
        MAX_SENTINEL_RECORDS as u64,
        records.len() as u64,
    );
    facts.bound(
        DOMAIN,
        "import-batch",
        MAX_SENTINEL_BATCH as u64,
        records.len() as u64,
    );
    facts.rule(DOMAIN, "fields", "repository-validated");
    facts.count(DOMAIN, "stages", records.len() as u64);
    facts.count(DOMAIN, "transitions", transitions);
    facts.count(DOMAIN, "handler-results", handled);
    facts.count(DOMAIN, "unconfirmed-claims", unconfirmed);
    Ok(())
}

/// Counts Sentinel cron handlers by whether the imported cron domain still holds the job;
/// targets are historical identities, so an absent job is recorded, never created.
pub(super) fn cross_relations(domains: &[DomainImport], facts: &mut Facts) {
    let mut live = HashSet::new();
    let mut retired = HashSet::new();
    let mut records: &[SentinelImport] = &[];
    for domain in domains {
        match domain {
            DomainImport::Cron(import) => {
                live.extend(import.jobs.iter().map(|job| job.record.id.as_str()));
                retired.extend(
                    import
                        .histories
                        .iter()
                        .map(|history| history.job_id.as_str()),
                );
            }
            DomainImport::Desk(_) => {}
            DomainImport::Sentinel(found) => records = found.as_slice(),
        }
    }
    let (mut to_live, mut to_retired, mut absent) = (0_u64, 0_u64, 0_u64);
    let handlers = records
        .iter()
        .flat_map(|record| &record.definition.buckets)
        .flat_map(|bucket| &bucket.handlers);
    for handler in handlers {
        let HandlerAction::Cron(target) = &handler.action else {
            continue;
        };
        if live.contains(target.as_str()) {
            to_live += 1;
        } else if retired.contains(target.as_str()) {
            to_retired += 1;
        } else {
            absent += 1;
        }
    }
    facts.count(DOMAIN, "cron-targets-live", to_live);
    facts.count(DOMAIN, "cron-targets-retired", to_retired);
    facts.count(DOMAIN, "cron-targets-absent", absent);
}

pub(super) fn occupied(conn: &Connection) -> Result<bool, StorageError> {
    conn.query_row("SELECT EXISTS (SELECT 1 FROM blueprints)", [], |row| {
        row.get(0)
    })
    .map_err(|e| sqlite_error("inspect sentinel domain", e))
}

/// A fenced outcome wrote nothing, but it still refuses the whole import.
pub(super) fn write(
    cx: &mut WriteCx<'_>,
    records: &[SentinelImport],
) -> Result<SentinelCommit, Halt> {
    let commit = runtime::import(cx, records.to_vec())?;
    match commit.outcome {
        MutationOutcome::Applied => Ok(commit),
        outcome => Err(Halt::Refused(ImportRefusal::Fenced {
            domain: DOMAIN,
            detail: format!("{outcome:?}"),
        })),
    }
}

pub(super) fn verify(
    records: &[SentinelImport],
    commit: &SentinelCommit,
) -> Result<(), ImportRefusal> {
    let actual: Vec<&SentinelRecord> = commit
        .sentinels
        .iter()
        .map(|stored| &stored.record)
        .collect();
    let mut diff = Diff::new(DOMAIN);
    compare::diff_sentinel(records, &actual, &mut diff);
    diff.gate()
}
