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
        .filter(|root| root.is_scenario_root() && one_shot(root))
        .filter(|root| {
            jobs.iter()
                .filter(|job| job.id == root.id || job.scenario_id.as_deref() == Some(&root.id))
                .all(|job| job.machine.stopped && !leased.contains(&job.id))
        })
        .cloned()
        .collect()
}

pub fn remove_terminal(store: &Arc<Mutex<Store>>) -> Result<(), String> {
    let roots = {
        let current = store.lock();
        current.ensure_available()?;
        terminal_roots(current.list(), &current.active.keys().cloned().collect())
    };
    remove(store, roots, false)
}

pub fn remove_owned(store: &Arc<Mutex<Store>>, tab: &str) -> Result<(), String> {
    let roots = {
        let current = store.lock();
        current.ensure_available()?;
        current
            .list()
            .iter()
            .filter(|job| job.is_scenario_root() && job.creator_tab_id == tab && one_shot(job))
            .cloned()
            .collect()
    };
    remove(store, roots, true)
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
