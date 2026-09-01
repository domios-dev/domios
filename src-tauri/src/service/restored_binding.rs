//! Re-establish a restarted tab's plan/task caller binding before it continues.
//! Every lookup is keyed on the tab's own record — never a global or latest
//! session.

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

/// Why a restore did not produce a binding. Branch on the discriminant,
/// never on the formatted message — that's prose for logs/CLI output only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RestoreBindingError {
    TabNotFound { tab_id: String },
    /// A `None` live session (not yet reported) is intentionally excluded —
    /// see `restore_caller_binding`.
    ForeignSession {
        tab_id: String,
        expected: String,
        live: String,
    },
    RebindFailed(String),
}

impl std::fmt::Display for RestoreBindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TabNotFound { tab_id } => {
                write!(f, "Tab {tab_id} not found while restoring its binding")
            }
            Self::ForeignSession {
                tab_id,
                expected,
                live,
            } => write!(
                f,
                "Tab {tab_id} came back on session {live} but its authoritative session is \
{expected}; refusing to rebind plan/task callers to a foreign session"
            ),
            Self::RebindFailed(message) => write!(f, "{message}"),
        }
    }
}

impl ServiceContext {
    /// A genuinely different session is refused as foreign; an unreported
    /// one (`None`) is not yet known, not wrong, so it settles untouched.
    pub(crate) fn restore_caller_binding(
        &self,
        tab_id: &str,
        expected_session_id: Option<&str>,
    ) -> Result<RestoredCallerBinding, RestoreBindingError> {
        let (live_session_id, title) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id).ok_or_else(|| RestoreBindingError::TabNotFound {
                tab_id: tab_id.to_string(),
            })?;
            (tab.agent_session_id.clone(), tab.title.clone())
        };
        if let Some(expected) = expected_session_id {
            match live_session_id.as_deref() {
                Some(live) if live != expected => {
                    return Err(RestoreBindingError::ForeignSession {
                        tab_id: tab_id.to_string(),
                        expected: expected.to_string(),
                        live: live.to_string(),
                    });
                }
                None => {
                    return Ok(RestoredCallerBinding {
                        tab_id: tab_id.to_string(),
                        agent_session_id: None,
                        ..Default::default()
                    });
                }
                Some(_) => {}
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
                        RestoreBindingError::RebindFailed(format!(
                            "owner rebind failed for plan {}: {error}",
                            plan.id
                        ))
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
                            RestoreBindingError::RebindFailed(format!(
                                "planner rebind failed for plan {}: {error}",
                                plan.id
                            ))
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

#[cfg(test)]
mod tests;
