use std::collections::HashMap;
use std::sync::Arc;

use super::ServiceContext;
use crate::agent::status::TabStatus;

fn status_transition(previous: Option<TabStatus>, current: TabStatus) -> Option<TabStatus> {
    (previous != Some(current)).then_some(current)
}

fn publish_statuses(
    previous: &mut HashMap<String, TabStatus>,
    current: &HashMap<String, TabStatus>,
    mut publish: impl FnMut(&str, serde_json::Value),
) {
    previous.retain(|tab_id, _| current.contains_key(tab_id));
    for (tab_id, status) in current {
        if let Some(status) = status_transition(previous.get(tab_id).copied(), *status) {
            previous.insert(tab_id.clone(), status);
            log::debug!("[status-publish] tab={tab_id} authoritative={status:?}");
            publish(
                "tab-status-changed",
                serde_json::json!({
                    "tab_id": tab_id,
                    "status": format!("{status:?}"),
                }),
            );
        }
    }
}

impl ServiceContext {
    pub(crate) fn publish_authoritative_tab_statuses(&self) {
        // Serialize snapshot capture with publication so concurrent producers cannot publish stale state.
        let mut published = self.published_tab_statuses.lock();
        let Some(statuses) = super::status_reconciliation::try_reconciled_tab_statuses(
            &self.mux,
            &self.agent_tracker,
            &self.agent_registry,
        ) else {
            drop(published);
            Arc::new(self.clone()).emit_state_changed_when_snapshot_readable();
            return;
        };
        publish_statuses(&mut published, &statuses, |event, payload| {
            crate::api::events::emit_everywhere(&self.app_handle, event, payload);
        });
    }
}

#[cfg(test)]
mod tests;
