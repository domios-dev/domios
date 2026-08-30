use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum InventoryOperation {
    Get,
    History,
    UpdateSchedule,
    Delete,
    Enable,
    Disable,
    Trigger,
    Cancel,
}

impl InventoryOperation {
    pub const fn all() -> [Self; 8] {
        [
            Self::Get,
            Self::History,
            Self::UpdateSchedule,
            Self::Delete,
            Self::Enable,
            Self::Disable,
            Self::Trigger,
            Self::Cancel,
        ]
    }

    pub const fn is_mutating(self) -> bool {
        matches!(
            self,
            Self::UpdateSchedule
                | Self::Delete
                | Self::Enable
                | Self::Disable
                | Self::Trigger
                | Self::Cancel
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InventoryFreshness {
    Fresh,
    Stale,
    Offline,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InventoryUnavailableReason {
    NotAuthorized,
    Stale,
    Offline,
    PartialPeerFailure,
    UnsupportedResourceKind,
    TargetUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InventoryCapability {
    pub allowed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<InventoryUnavailableReason>,
}

impl InventoryCapability {
    pub const fn allowed() -> Self {
        Self {
            allowed: true,
            unavailable_reason: None,
        }
    }

    pub const fn unavailable(reason: InventoryUnavailableReason) -> Self {
        Self {
            allowed: false,
            unavailable_reason: Some(reason),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InventoryProvenance {
    pub freshness: InventoryFreshness,
    pub offline: bool,
    pub fetched_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InventoryCapabilityMap {
    pub operations: BTreeMap<InventoryOperation, InventoryCapability>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_keys_are_stable_wire_names() {
        let mut operations = BTreeMap::new();
        operations.insert(
            InventoryOperation::UpdateSchedule,
            InventoryCapability::unavailable(InventoryUnavailableReason::NotAuthorized),
        );
        let value = serde_json::to_value(InventoryCapabilityMap { operations }).unwrap();
        assert_eq!(
            value["operations"]["update_schedule"]["unavailable_reason"],
            "not_authorized"
        );
    }
}
