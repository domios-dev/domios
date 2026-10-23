use crate::verify::{Refusal, VerifiedGrant};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Durable revision high-water for one account and installation; it only moves up.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionFloor {
    pub revision: u64,
    pub digest: String,
}

impl RevisionFloor {
    pub fn of(grant: &VerifiedGrant) -> Self {
        Self {
            revision: grant.payload().revision,
            digest: digest(grant.canonical_bytes()),
        }
    }

    /// A lower revision, or the same revision with other signed bytes, is a rollback.
    pub fn admits(&self, grant: &VerifiedGrant) -> Result<(), Refusal> {
        let revision = grant.payload().revision;
        if revision < self.revision {
            Err(Refusal::Revision)
        } else if revision == self.revision && digest(grant.canonical_bytes()) != self.digest {
            Err(Refusal::RevisionConflict)
        } else {
            Ok(())
        }
    }

    pub fn raised(floor: Option<&Self>, grant: &VerifiedGrant) -> Self {
        match floor {
            Some(floor) if floor.revision > grant.payload().revision => floor.clone(),
            _ => Self::of(grant),
        }
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
