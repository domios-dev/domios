//! Advisory workspace tab-budget nudges (task_4d649dfd). Non-blocking: a
//! workspace that accumulates stale tabs (esp. after orchestration waves)
//! gets a visible cleanup nudge on the next tab-creation response. Never
//! blocks creation, never auto-closes tabs.

use serde::Serialize;

use super::ServiceContext;

/// Soft budget: past this the creation response carries a gentle nudge.
pub const SOFT_TAB_BUDGET: usize = 15;
/// Strong budget: past this the response is visibly not a plain OK and names
/// the exact count.
pub const STRONG_TAB_BUDGET: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TabBudgetLevel {
    Soft,
    Strong,
}

/// Structured advisory attached to any tab-creation response so a future UI
/// can render the same nudge the CLI/API text shows. `message` is the
/// human-readable line; `workspace_tab_count` is the post-create count for
/// THIS workspace only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TabBudgetNotice {
    pub workspace_tab_count: usize,
    pub level: TabBudgetLevel,
    pub message: String,
}

/// Evaluate a post-create workspace tab count against the advisory budgets.
/// `None` when under the soft threshold — no nudge. Pure so CLI/socket/UI
/// paths can't drift and it is trivially unit-testable.
pub fn evaluate_tab_budget(count: usize) -> Option<TabBudgetNotice> {
    if count > STRONG_TAB_BUDGET {
        Some(TabBudgetNotice {
            workspace_tab_count: count,
            level: TabBudgetLevel::Strong,
            message: format!(
                "Workspace now has {count} Tabs. Are you sure you did not forget to close finished Tabs?"
            ),
        })
    } else if count > SOFT_TAB_BUDGET {
        Some(TabBudgetNotice {
            workspace_tab_count: count,
            level: TabBudgetLevel::Soft,
            message: format!(
                "Workspace now has {count} Tabs — consider closing finished Tabs you no longer need."
            ),
        })
    } else {
        None
    }
}

impl ServiceContext {
    /// THE shared entry point every tab-creation path uses to compute its
    /// advisory nudge. Reads the live count for `workspace_id` only (other
    /// workspaces never contribute) and evaluates it against the budgets.
    pub fn workspace_tab_budget_notice(&self, workspace_id: &str) -> Option<TabBudgetNotice> {
        let count = self.mux.read().tab_count_in_workspace(workspace_id);
        evaluate_tab_budget(count)
    }
}

#[cfg(test)]
#[path = "tab_budget_tests.rs"]
mod tests;
