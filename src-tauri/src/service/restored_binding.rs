//! Re-establish a restarted tab's authoritative identity before it continues.
//!
//! A restart replaces the PTY, not the tab. Everything the plan layer points at
//! — owner ref, planner ref, open execution attempts — still names this tab, and
//! those refs carry the session id the tab held before the restart. Until they
//! agree again, a continuation would run against a stale caller binding.
//!
//! Every lookup here is keyed on the tab's OWN record. There is no global or
//! "latest session" fallback: an unexpected session id fails the restore rather
//! than being adopted.

use crate::plan::PlannerRef;

use super::ServiceContext;

/// What a restarted tab regained. Reported so restart callers can assert that
/// binding was restored *before* readiness is handed back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RestoredCallerBinding {
    pub tab_id: String,
    pub agent_session_id: Option<String>,
    pub owned_plan_ids: Vec<String>,
    pub planner_plan_ids: Vec<String>,
    pub task_bindings: Vec<(String, String)>,
}

impl RestoredCallerBinding {
    pub fn is_empty(&self) -> bool {
        self.owned_plan_ids.is_empty()
            && self.planner_plan_ids.is_empty()
            && self.task_bindings.is_empty()
    }
}

impl ServiceContext {
    /// Restore the exact plan/task caller binding for one restarted tab.
    ///
    /// `expected_session_id` is the session captured before the restart. When
    /// the restart resumed that session, the live tab must still hold it; a
    /// different id means the tab was rebound to a foreign session and the
    /// restore fails instead of silently re-pointing plan refs at it.
    pub(crate) fn restore_caller_binding(
        &self,
        tab_id: &str,
        expected_session_id: Option<&str>,
    ) -> Result<RestoredCallerBinding, String> {
        let (live_session_id, title) = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or_else(|| format!("Tab {tab_id} not found while restoring its binding"))?;
            (tab.agent_session_id.clone(), tab.title.clone())
        };
        if let Some(expected) = expected_session_id {
            if live_session_id.as_deref() != Some(expected) {
                return Err(format!(
                    "Tab {tab_id} came back on session {} but its authoritative session is {expected}; \
refusing to rebind plan/task callers to a foreign session",
                    live_session_id.as_deref().unwrap_or("<none>")
                ));
            }
        }

        let mut restored = RestoredCallerBinding {
            tab_id: tab_id.to_string(),
            agent_session_id: live_session_id.clone(),
            ..Default::default()
        };
        let now_ms = epoch_ms();

        for plan in self.plan_store.list_full() {
            if plan.owner.tab_id.as_deref() == Some(tab_id) {
                self.plan_store
                    .refresh_owner_session(
                        &plan.id,
                        tab_id,
                        live_session_id.as_deref(),
                        Some(&title),
                        now_ms,
                    )
                    .map_err(|error| {
                        format!("owner rebind failed for plan {}: {error}", plan.id)
                    })?;
                restored.owned_plan_ids.push(plan.id.clone());
            }
            let planner_is_this_tab = matches!(
                &plan.planner,
                Some(PlannerRef::Tab { tab_id: bound, .. }) if bound == tab_id
            );
            if planner_is_this_tab {
                let refreshed = PlannerRef::Tab {
                    tab_id: tab_id.to_string(),
                    session_id: live_session_id.clone(),
                    title: Some(title.clone()),
                };
                if plan.planner.as_ref() != Some(&refreshed) {
                    self.plan_store
                        .set_planner_ref(&plan.id, Some(refreshed), now_ms)
                        .map_err(|error| {
                            format!("planner rebind failed for plan {}: {error}", plan.id)
                        })?;
                }
                restored.planner_plan_ids.push(plan.id.clone());
            }
        }

        restored.task_bindings = self.plan_store.task_bindings_for_tab(tab_id);
        restored.owned_plan_ids.sort();
        restored.planner_plan_ids.sort();
        restored.task_bindings.sort();

        let changed: std::collections::BTreeSet<&String> = restored
            .owned_plan_ids
            .iter()
            .chain(restored.planner_plan_ids.iter())
            .collect();
        for plan_id in changed {
            self.emit_plan_changed(plan_id);
        }
        Ok(restored)
    }
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
