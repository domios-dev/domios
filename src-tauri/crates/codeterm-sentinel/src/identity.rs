use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::store::Sentinel;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    #[serde(deserialize_with = "identity_kind")]
    pub kind: String,
    pub id: String,
    pub workspace_id: String,
    pub team_id: Option<String>,
    pub owner_tab_id: Option<String>,
}

fn identity_kind<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let kind = String::deserialize(deserializer)?;
    Ok(if kind == "sentinel" {
        "blueprint".into()
    } else {
        kind
    })
}

pub fn identity(job: &Sentinel) -> Identity {
    Identity {
        kind: "blueprint".into(),
        id: job.id.clone(),
        workspace_id: job.workspace_id.clone(),
        team_id: job.team_id.clone(),
        owner_tab_id: Some(job.creator_tab_id.clone()),
    }
}

pub fn validate_scope(
    mux: &MultiplexerState,
    workspace: &str,
    team: Option<&str>,
) -> Result<(), String> {
    if mux.get_workspace(workspace).is_none() {
        return Err("automation workspace not found".into());
    }
    if let Some(team) = team {
        if mux.get_team(team).is_none() || mux.team_workspace(team).as_deref() != Some(workspace) {
            return Err("automation Team does not belong to the current workspace".into());
        }
    }
    Ok(())
}

#[cfg(test)]
pub fn scoped_inventory(
    store: &super::store::Store,
    mux: &MultiplexerState,
    workspace: &str,
    team: Option<&str>,
) -> Result<Vec<Sentinel>, String> {
    validate_scope(mux, workspace, team)?;
    store.ensure_available()?;
    Ok(store
        .list()
        .iter()
        .filter(|job| job.workspace_id == workspace && job.team_id.as_deref() == team)
        .cloned()
        .collect())
}

pub fn reachable_ids(jobs: &[Sentinel]) -> BTreeSet<String> {
    let mut pending: Vec<_> = jobs
        .iter()
        .filter(|job| job.is_scenario_root())
        .map(|job| job.id.clone())
        .collect();
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(source) = jobs.iter().find(|job| job.id == id) else {
            continue;
        };
        pending.extend(source.routes.values().flatten().filter_map(|handler| {
            if let Route::Sentinel(target) = &handler.action {
                jobs.iter()
                    .find(|job| job.id == *target && job.same_owner(source))
                    .map(|job| job.id.clone())
            } else {
                None
            }
        }));
    }
    seen
}

pub fn host_delete_staged(
    store: &mut Store,
    mux: &MultiplexerState,
    workspace: &str,
    team: Option<&str>,
    id: &str,
) -> Result<Staged<()>, String> {
    validate_scope(mux, workspace, team)?;
    store.ensure_available()?;
    let root = store.get(id).ok_or("Blueprint scenario not found")?;
    if root.workspace_id != workspace
        || root.team_id.as_deref() != team
        || !root.is_scenario_root()
        || !reachable_ids(store.list()).contains(id)
    {
        return Err("Blueprint scenario is unavailable in this automation scope".into());
    }
    Ok(store.stage_delete_scenario(id, workspace, team))
}

pub fn scope_revision(jobs: &[Sentinel], workspace: &str, team: Option<&str>) -> String {
    let rows: Vec<_> = jobs
        .iter()
        .filter(|j| j.workspace_id == workspace && j.team_id.as_deref() == team)
        .map(|j| {
            json!([
                j.id,
                j.revision,
                j.agent_authored,
                j.model_classification,
                j.name,
                j.scenario_id,
                j.machine.paused,
                j.machine.stopped,
                j.machine.last_bucket,
                j.last_reason,
                j.progress,
                j.routes,
                j.transitions,
                j.expires_at,
                j.next_run,
                j.input().cloned()
            ])
        })
        .collect();
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&rows).unwrap_or_default())
    )
}
