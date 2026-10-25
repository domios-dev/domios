//! Load-time quarantine: one definition that fails validation is set aside, never the store.

use serde::Serialize;

use crate::store::{validate, Sentinel};

pub const INVALID_DEFINITION: &str = "blueprint_quarantined_invalid_definition";
pub const ORPHANED_STAGE: &str = "blueprint_quarantined_scenario_root_unavailable";

/// A committed Blueprint refused at load; it stays in Storage untouched and never runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Quarantined {
    pub id: String,
    pub name: Option<String>,
    pub scenario_id: Option<String>,
    pub workspace_id: String,
    pub team_id: Option<String>,
    pub creator_tab_id: String,
    pub state: &'static str,
    pub code: &'static str,
    pub reason: String,
}

impl Quarantined {
    fn new(job: &Sentinel, code: &'static str, reason: String) -> Self {
        Self {
            id: job.id.clone(),
            name: job.name.clone(),
            scenario_id: job.scenario_id.clone(),
            workspace_id: job.workspace_id.clone(),
            team_id: job.team_id.clone(),
            creator_tab_id: job.creator_tab_id.clone(),
            state: "quarantined",
            code,
            reason,
        }
    }

    pub fn same_scope(&self, workspace: &str, team: Option<&str>) -> bool {
        self.workspace_id == workspace && self.team_id.as_deref() == team
    }
}

/// Splits a committed inventory into runnable stages and quarantined ones. Only an
/// inventory-wide invariant (capacity, duplicate identity) still refuses the whole load.
pub fn partition(jobs: Vec<Sentinel>) -> Result<(Vec<Sentinel>, Vec<Quarantined>), String> {
    let mut quarantined = Vec::new();
    let mut kept = Vec::with_capacity(jobs.len());
    for job in &jobs {
        let root = job
            .scenario_id
            .as_deref()
            .filter(|root| *root != job.id)
            .and_then(|root| jobs.iter().find(|candidate| candidate.id == root));
        let alone = std::iter::once(job.clone()).chain(root.cloned()).collect();
        match validate(alone) {
            Ok(_) => kept.push(job.clone()),
            Err(reason) => quarantined.push(Quarantined::new(job, INVALID_DEFINITION, reason)),
        }
    }
    loop {
        let orphans: Vec<usize> = kept
            .iter()
            .enumerate()
            .filter(|(_, job)| {
                job.scenario_id.as_deref().is_some_and(|root| {
                    root != job.id && !kept.iter().any(|candidate| candidate.id == root)
                })
            })
            .map(|(index, _)| index)
            .collect();
        if orphans.is_empty() {
            break;
        }
        for index in orphans.into_iter().rev() {
            let job = kept.remove(index);
            let reason = format!(
                "scenario root {} is quarantined or missing",
                job.scenario_id.as_deref().unwrap_or_default()
            );
            quarantined.push(Quarantined::new(&job, ORPHANED_STAGE, reason));
        }
    }
    Ok((validate(kept)?, quarantined))
}
