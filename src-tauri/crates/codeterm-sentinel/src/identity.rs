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
