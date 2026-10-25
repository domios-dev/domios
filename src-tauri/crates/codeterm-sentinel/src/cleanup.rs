use std::collections::BTreeSet;
use std::sync::Arc;

use parking_lot::Mutex;

use super::{
    machine::Route,
    storage,
    store::{Sentinel, Store},
};

pub fn one_shot(job: &Sentinel) -> bool {
    job.pipeline().is_none()
        && job.rearm_on_change.is_empty()
        && (job.until.is_some()
            || job
                .routes
                .values()
                .flatten()
                .any(|handler| handler.action == Route::Stop))
}

pub fn terminal_roots(jobs: &[Sentinel], leased: &BTreeSet<String>) -> Vec<Sentinel> {
    jobs.iter()
        .filter(|root| root.is_scenario_root() && one_shot(root) && !retain_for_team(root))
        .filter(|root| terminal_graph(jobs, root, leased))
        .cloned()
        .collect()
}

pub(crate) fn oldest_capacity_candidate(
    jobs: &[Sentinel],
    leased: &BTreeSet<String>,
    excluded: &BTreeSet<String>,
) -> Option<Sentinel> {
    jobs.iter()
        .find(|root| {
            root.is_scenario_root()
                && one_shot(root)
                && retain_for_team(root)
                && terminal_graph(jobs, root, leased)
                && !jobs.iter().any(|job| {
                    (job.id == root.id || job.scenario_id.as_deref() == Some(&root.id))
                        && excluded.contains(&job.id)
                })
        })
        .cloned()
}

fn terminal_graph(jobs: &[Sentinel], root: &Sentinel, leased: &BTreeSet<String>) -> bool {
    jobs.iter()
        .filter(|job| job.id == root.id || job.scenario_id.as_deref() == Some(&root.id))
        .all(|job| {
            job.admission.permits_steps() && job.machine.stopped && !leased.contains(&job.id)
        })
}

pub fn remove_terminal(store: &Arc<Mutex<Store>>) -> Result<(), String> {
    let missing_times = store
        .lock()
        .list()
        .iter()
        .filter(|job| job.machine.stopped && job.cleanup.stopped_at.is_none())
        .map(|job| job.id.clone())
        .collect::<Vec<_>>();
    for id in missing_times {
        storage::commit(store, |current| {
            if !current
                .get(&id)
                .is_some_and(|job| job.machine.stopped && job.cleanup.stopped_at.is_none())
            {
                return storage::Staged::ready(Ok(()));
            }
            let reason = current
                .get(&id)
                .and_then(|job| job.last_reason.clone())
                .unwrap_or_else(|| "stopped".into());
            current.stage_stop(&id, &reason).map(|_| ())
        })?;
    }
    let roots = {
        let current = store.lock();
        current.ensure_available()?;
        expired_roots(
            current.list(),
            &current.active.keys().cloned().collect(),
            current.clock.now_ms(),
        )
    };
    for root in roots {
        storage::commit(store, |current| {
            let eligible = expired_roots(
                current.list(),
                &current.active.keys().cloned().collect(),
                current.clock.now_ms(),
            )
            .iter()
            .any(|job| job.id == root.id && job.same_owner(&root));
            if !eligible {
                return storage::Staged::ready(Ok(()));
            }
            let caller = super::delete::DeleteCaller {
                tab: &root.creator_tab_id,
                workspace: &root.workspace_id,
                team: root.team_id.as_deref(),
                team_owner: false,
            };
            match super::delete::stage_delete(
                current,
                &caller,
                super::delete::DeleteTarget::One(&root.id),
                false,
            ) {
                Ok(mut staged) => staged
                    .pop()
                    .map(|(_, staged)| staged)
                    .unwrap_or_else(|| storage::Staged::ready(Ok(()))),
                Err(error) => storage::Staged::ready(Err(error)),
            }
        })?;
    }
    Ok(())
}

pub fn remove_owned(store: &Arc<Mutex<Store>>, tab: &str) -> Result<(), String> {
    let roots = {
        let current = store.lock();
        current.ensure_available()?;
        current
            .list()
            .iter()
            .filter(|job| {
                job.is_scenario_root()
                    && job.creator_tab_id == tab
                    && one_shot(job)
                    && !retain_for_team(job)
            })
            .cloned()
            .collect()
    };
    remove(store, roots, true)
}

fn retain_for_team(job: &Sentinel) -> bool {
    job.agent_authored && job.team_id.is_some()
}

fn remove(store: &Arc<Mutex<Store>>, roots: Vec<Sentinel>, owned: bool) -> Result<(), String> {
    for root in roots {
        storage::commit(store, |current| {
            let eligible = current.get(&root.id).is_some_and(|job| {
                job.same_owner(&root)
                    && job.definition_version == root.definition_version
                    && one_shot(job)
            }) && (owned
                || terminal_roots(current.list(), &current.active.keys().cloned().collect())
                    .iter()
                    .any(|job| job.id == root.id));
            if !eligible {
                return storage::Staged::ready(Ok(()));
            }
            current.stage_delete_scenario(&root.id, &root.workspace_id, root.team_id.as_deref())
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

pub fn deletion_deadline(
    jobs: &[Sentinel],
    root: &Sentinel,
    leased: &BTreeSet<String>,
) -> Option<u64> {
    if root.scenario_id.as_deref().is_some_and(|id| id != root.id) {
        return None;
    }
    let members = jobs
        .iter()
        .filter(|job| {
            job.workspace_id == root.workspace_id
                && job.team_id == root.team_id
                && (job.id == root.id || job.scenario_id.as_deref() == Some(&root.id))
        })
        .collect::<Vec<_>>();
    if members.iter().any(|job| {
        !job.admission.permits_steps() || !job.machine.stopped || leased.contains(&job.id)
    }) {
        return None;
    }
    members
        .iter()
        .map(|job| job.cleanup.deletes_at)
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .max()
}

fn expired_roots(jobs: &[Sentinel], leased: &BTreeSet<String>, now: u64) -> Vec<Sentinel> {
    jobs.iter()
        .filter(|root| deletion_deadline(jobs, root, leased).is_some_and(|at| now >= at))
        .cloned()
        .collect()
}
