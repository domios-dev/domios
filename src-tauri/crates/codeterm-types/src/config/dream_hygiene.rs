use serde::{Deserialize, Serialize};

/// Reversible Dreamer hygiene: nightly merge and stale-archive budgets.
/// The stale archive stays off until the owner has spot-checked a dry run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DreamHygieneConfig {
    pub stale_archive: bool,
    pub stale_archive_cap: u32,
    pub merge_cap: u32,
}

impl Default for DreamHygieneConfig {
    fn default() -> Self {
        Self {
            stale_archive: false,
            stale_archive_cap: 20,
            merge_cap: 10,
        }
    }
}
