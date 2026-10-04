//! Versioned definition lineage: each amendment names the head it replaced and its author.

use serde::{Deserialize, Serialize};

use super::source::bounded_id;
use super::store::Sentinel;

/// Retained lineage window, equal to the newest amended versions typed Storage reads back.
pub const MAX_AMENDMENTS: usize = crate::storage::MAX_SENTINEL_LINEAGE;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Amendment {
    pub version: u64,
    pub replaced_version: u64,
    pub author_tab_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause_transition_id: Option<String>,
    pub at: u64,
}

pub fn initial_version() -> u64 {
    1
}

pub(super) fn is_initial_version(version: &u64) -> bool {
    *version == 1
}

/// Versions rise strictly up to the head, each replacing an earlier one, and a later head
/// carries its own lineage last.
pub(super) fn validate(job: &Sentinel) -> Result<(), String> {
    let mut previous = 1;
    for amendment in &job.amendments {
        if amendment.version <= previous
            || amendment.version > job.definition_version
            || amendment.replaced_version == 0
            || amendment.replaced_version >= amendment.version
            || !bounded_id(&amendment.author_tab_id)
            || amendment
                .cause_transition_id
                .as_deref()
                .is_some_and(|cause| !bounded_id(cause))
        {
            return Err("invalid sentinel amendment lineage".into());
        }
        previous = amendment.version;
    }
    let head_lineage = job.amendments.last().map(|amendment| amendment.version);
    let expected = (job.definition_version > 1).then_some(job.definition_version);
    if job.definition_version == 0
        || job.amendments.len() > MAX_AMENDMENTS
        || head_lineage != expected
    {
        return Err("invalid sentinel definition version".into());
    }
    Ok(())
}
