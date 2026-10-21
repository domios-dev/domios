use serde::{Deserialize, Serialize};

pub fn can_review_tab_todo(actor: &str, parent: Option<&str>, plan_owner: Option<&str>) -> bool {
    !actor.is_empty() && (parent == Some(actor) || plan_owner == Some(actor))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabTodoProposalState {
    Proposed,
    Approved,
    Amended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoProposal {
    pub state: TabTodoProposalState,
    pub shared_at: u64,
    pub reviewed_at: Option<u64>,
    pub reviewer_tab_id: Option<String>,
    pub note: Option<String>,
}

impl TabTodoProposal {
    pub fn shared(at: u64) -> Self {
        Self {
            state: TabTodoProposalState::Proposed,
            shared_at: at,
            reviewed_at: None,
            reviewer_tab_id: None,
            note: None,
        }
    }

    pub fn review(&mut self, amended: bool, actor: Option<String>, note: Option<String>, at: u64) {
        self.state = if amended {
            TabTodoProposalState::Amended
        } else {
            TabTodoProposalState::Approved
        };
        self.reviewed_at = Some(at);
        self.reviewer_tab_id = actor;
        self.note = note;
    }
}

#[cfg(test)]
mod tests;
