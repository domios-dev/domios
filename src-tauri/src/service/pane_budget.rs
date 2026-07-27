//! Advisory workspace pane-budget nudges (task_4d649dfd). Non-blocking: a
//! workspace that accumulates stale panes (esp. after orchestration waves)
//! gets a visible cleanup nudge on the next pane-creation response. Never
//! blocks creation, never auto-closes panes.

use serde::Serialize;

use super::ServiceContext;

/// Soft budget: past this the creation response carries a gentle nudge.
pub const SOFT_PANE_BUDGET: usize = 15;
/// Strong budget: past this the response is visibly not a plain OK and names
/// the exact count.
pub const STRONG_PANE_BUDGET: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneBudgetLevel {
    Soft,
    Strong,
}

/// Structured advisory attached to any pane-creation response so a future UI
/// can render the same nudge the CLI/API text shows. `message` is the
/// human-readable line; `workspace_pane_count` is the post-create count for
/// THIS workspace only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaneBudgetNotice {
    pub workspace_pane_count: usize,
    pub level: PaneBudgetLevel,
    pub message: String,
}

/// Evaluate a post-create workspace pane count against the advisory budgets.
/// `None` when under the soft threshold — no nudge. Pure so CLI/socket/UI
/// paths can't drift and it is trivially unit-testable.
pub fn evaluate_pane_budget(count: usize) -> Option<PaneBudgetNotice> {
    if count > STRONG_PANE_BUDGET {
        Some(PaneBudgetNotice {
            workspace_pane_count: count,
            level: PaneBudgetLevel::Strong,
            message: format!(
                "Workspace now has {count} Tabs. Are you sure you did not forget to close finished Tabs?"
            ),
        })
    } else if count > SOFT_PANE_BUDGET {
        Some(PaneBudgetNotice {
            workspace_pane_count: count,
            level: PaneBudgetLevel::Soft,
            message: format!(
                "Workspace now has {count} Tabs — consider closing finished Tabs you no longer need."
            ),
        })
    } else {
        None
    }
}

impl ServiceContext {
    /// THE shared entry point every pane-creation path uses to compute its
    /// advisory nudge. Reads the live count for `workspace_id` only (other
    /// workspaces never contribute) and evaluates it against the budgets.
    pub fn workspace_pane_budget_notice(&self, workspace_id: &str) -> Option<PaneBudgetNotice> {
        let count = self.mux.read().pane_count_in_workspace(workspace_id);
        evaluate_pane_budget(count)
    }
}

#[cfg(test)]
#[path = "pane_budget_tests.rs"]
mod tests;
