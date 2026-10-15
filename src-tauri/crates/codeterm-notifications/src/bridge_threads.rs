use codeterm_bridge::bridge::threads::{SenderIdentity, SenderRole};

pub struct TabIdentity {
    pub id: String,
    pub title: String,
    pub role: Option<String>,
    pub workspace: String,
    pub is_general: bool,
}

pub fn sender_identity(tab: TabIdentity, machine: String) -> SenderIdentity {
    SenderIdentity {
        tab_id: tab.id,
        title: tab.title,
        role: SenderRole::classify(tab.is_general, tab.role.as_deref()),
        workspace: tab.workspace,
        machine,
    }
}
