use serde::Deserialize;

use crate::{mem::store, service::ServiceState};

#[derive(Deserialize)]
pub struct AreaMergeBody {
    pub from: String,
    pub into: String,
    #[serde(default)]
    pub dry_run: bool,
    pub worker_id: Option<String>,
}

pub(crate) fn merge_area(
    svc: &ServiceState,
    body: &AreaMergeBody,
    worker_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    let intent = worker_id.and_then(|id| svc.workers.get(id).map(|worker| worker.intent));
    let dreaming = intent
        .as_deref()
        .is_some_and(crate::mem::workers::is_dreaming_intent);
    let actor = super::mem_merge::merge_actor(intent.as_deref(), None);
    let outcome = store::with_changed_by(&actor, || {
        svc.mem.with_conn(|conn| {
            store::area_merge::merge_area(
                conn,
                svc.mem.blobs_dir(),
                &body.from,
                &body.into,
                body.dry_run,
                if dreaming { worker_id } else { None },
            )
        })
    })?;
    if !body.dry_run {
        for id in &outcome.item_ids {
            svc.mem.refresh_embedding(id);
        }
    }
    serde_json::to_value(outcome).map_err(|e| e.to_string())
}
