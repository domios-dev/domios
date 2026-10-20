use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedTab {
    pub manager_tab_id: String,
    pub manager_name: String,
    pub manager_closed: bool,
}

impl ManagedTab {
    pub fn is_active(&self) -> bool {
        !self.manager_closed
    }
}
