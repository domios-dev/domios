use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NeighborAuthority {
    pub uid: String,
    pub friendship_id: String,
    /// None on the concierge itself; descendants retain the concierge's tab id.
    pub root_tab_id: Option<String>,
}

#[cfg(test)]
mod tests;
