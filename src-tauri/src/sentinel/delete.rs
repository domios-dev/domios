use std::collections::BTreeSet;

use super::storage::Staged;
use super::store::{Sentinel, Store};

/// Who asks to delete: the calling tab, its scope, and whether it anchors its Team.
pub(crate) struct DeleteCaller<'a> {
    pub tab: &'a str,
    pub workspace: &'a str,
    pub team: Option<&'a str>,
    pub team_owner: bool,
}

pub(crate) enum DeleteTarget<'a> {
    One(&'a str),
    Stopped,
}

fn root_of(job: &Sentinel) -> &str {
    job.scenario_id.as_deref().unwrap_or(&job.id)
}

fn members<'a>(jobs: &'a [Sentinel], root: &'a str) -> impl Iterator<Item = &'a Sentinel> {
    jobs.iter()
        .filter(move |job| job.id == root || job.scenario_id.as_deref() == Some(root))
}

fn may_delete(caller: &DeleteCaller, root: &Sentinel) -> bool {
    root.creator_tab_id == caller.tab || (caller.team_owner && caller.team.is_some())
}

/// The scenario roots one delete request removes; a stage resolves to its whole Blueprint.
pub(crate) fn plan_delete(
    jobs: &[Sentinel],
    caller: &DeleteCaller,
    target: DeleteTarget,
    stop: bool,
) -> Result<Vec<String>, String> {
    let scoped: Vec<Sentinel> = jobs
        .iter()
        .filter(|job| job.workspace_id == caller.workspace && job.team_id.as_deref() == caller.team)
        .cloned()
        .collect();
    let running = |root: &str| members(&scoped, root).any(|job| !job.machine.stopped);
    let root_job = |root: &str| scoped.iter().find(|job| job.id == root);
    match target {
        DeleteTarget::One(id) => {
            let job = scoped
                .iter()
                .find(|job| job.id == id)
                .ok_or_else(|| format!("blueprint not found in this Team: {id}"))?;
            let root = root_of(job).to_string();
            let owner = root_job(&root).unwrap_or(job);
            if !may_delete(caller, owner) {
                return Err(format!(
                    "only the creator tab or the Team owner may delete blueprint {root}"
                ));
            }
            if running(&root) && !stop {
                return Err(format!(
                    "blueprint {root} is running; stop it first or pass --stop"
                ));
            }
            Ok(vec![root])
        }
        DeleteTarget::Stopped => Ok(scoped
            .iter()
            .map(root_of)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|root| {
                root_job(root).is_some_and(|owner| may_delete(caller, owner)) && !running(root)
            })
            .map(str::to_string)
            .collect()),
    }
}

/// Stages every planned root's removal under one store lock; settle each with no lock held.
pub(crate) fn stage_delete(
    store: &mut Store,
    caller: &DeleteCaller,
    target: DeleteTarget,
    stop: bool,
) -> Result<Vec<(String, Staged<()>)>, String> {
    store.ensure_available()?;
    let roots = plan_delete(store.list(), caller, target, stop)?;
    Ok(roots
        .into_iter()
        .map(|root| {
            let staged = store.stage_delete_scenario(&root, caller.workspace, caller.team);
            (root, staged)
        })
        .collect())
}

#[cfg(test)]
#[path = "delete_tests.rs"]
mod tests;
