use super::record::{CommittedAuthority, LifecycleCheckpoint, LifecycleEpoch};

/// Authority a persisted snapshot claims for one tab, lifted out of the session
/// snapshot before anything is restored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotAuthorityClaim {
    pub tab_id: String,
    pub epoch: Option<LifecycleEpoch>,
    pub operation_id: Option<String>,
    pub provider: Option<String>,
    pub chat_backend: bool,
    pub session_id: Option<String>,
    pub binding_nonce: Option<String>,
    pub role: Option<String>,
    pub role_profile: Option<String>,
    pub team_id: Option<String>,
    pub parent_tab_id: Option<String>,
    pub can_spawn: bool,
    pub depth: u8,
}

impl SnapshotAuthorityClaim {
    /// True when the snapshot asserts anything a restarted tab could act on.
    pub fn claims_authority(&self) -> bool {
        self.provider.is_some()
            || self.chat_backend
            || self.session_id.is_some()
            || self.binding_nonce.is_some()
            || self.claims_role_authority()
    }

    /// Authority that only means something inside an orchestration hierarchy.
    pub fn claims_role_authority(&self) -> bool {
        self.role.is_some()
            || self.role_profile.is_some()
            || self.team_id.is_some()
            || self.parent_tab_id.is_some()
            || self.can_spawn
            || self.depth > 0
    }

    /// A tab enters journal coverage by carrying its own lifecycle lineage.
    /// Neither field means this tab's lifecycle predates the journal.
    pub fn lineage(&self) -> Lineage<'_> {
        match (self.epoch, self.operation_id.as_deref()) {
            (None, None) => Lineage::PreJournal,
            (Some(epoch), Some(operation_id)) => Lineage::Covered {
                epoch,
                operation_id,
            },
            _ => Lineage::Partial,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lineage<'a> {
    PreJournal,
    Partial,
    Covered {
        epoch: LifecycleEpoch,
        operation_id: &'a str,
    },
}

/// Why a claim was refused. Each variant names the missing proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryRefusal {
    /// Covered, but no sealed operation the journal still carries matches. A
    /// deleted or rebuilt journal reaches the tab this way.
    NoCommittedCheckpoint,
    /// One lineage field without the other. Half a lineage proves nothing.
    PartialLineage,
    /// Role or Team authority with nothing behind it.
    UnbackedRoleAuthority,
    EpochMismatch {
        snapshot: LifecycleEpoch,
        checkpoint: LifecycleEpoch,
    },
    OperationMismatch {
        snapshot: String,
        checkpoint: String,
    },
    FieldMismatch {
        field: &'static str,
    },
    /// The journal could not be replayed to its end, so no checkpoint it might
    /// hold can be trusted.
    JournalTailRefused {
        reason: String,
    },
}

impl RecoveryRefusal {
    pub fn reason(&self) -> String {
        match self {
            Self::NoCommittedCheckpoint => {
                "no committed lifecycle checkpoint for this tab operation".to_string()
            }
            Self::PartialLineage => "snapshot carries only half a lifecycle lineage".to_string(),
            Self::UnbackedRoleAuthority => {
                "role authority claimed without a provider identity".to_string()
            }
            Self::EpochMismatch {
                snapshot,
                checkpoint,
            } => format!("snapshot epoch {snapshot} is not checkpoint epoch {checkpoint}"),
            Self::OperationMismatch {
                snapshot,
                checkpoint,
            } => format!("snapshot operation {snapshot} is not checkpoint operation {checkpoint}"),
            Self::FieldMismatch { field } => {
                format!("snapshot {field} disagrees with the committed checkpoint")
            }
            Self::JournalTailRefused { reason } => {
                format!("lifecycle journal tail refused: {reason}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryVerdict {
    /// Nothing to prove — the tab claims no authority.
    NoAuthorityClaimed,
    /// No lineage, so the journal does not judge this tab. It keeps whatever
    /// the pre-journal contract already allowed.
    PreJournal,
    /// Every claimed field is backed by the exact committed checkpoint.
    Restored(Box<CommittedAuthority>),
    /// Authority stripped; the tab comes back as a plain shell.
    Stripped(RecoveryRefusal),
}

impl RecoveryVerdict {
    pub fn is_stripped(&self) -> bool {
        matches!(self, Self::Stripped(_))
    }
}

/// The whole recovery rule, applied per tab. A tab enters coverage only once
/// its own lifecycle operation is journaled.
pub fn authorize(
    claim: &SnapshotAuthorityClaim,
    checkpoint: Option<&LifecycleCheckpoint>,
    journal_tail_refusal: Option<&str>,
) -> RecoveryVerdict {
    if !claim.claims_authority() {
        return RecoveryVerdict::NoAuthorityClaimed;
    }
    let (epoch, operation_id) = match claim.lineage() {
        Lineage::PreJournal => return pre_journal_verdict(claim),
        Lineage::Partial => return RecoveryVerdict::Stripped(RecoveryRefusal::PartialLineage),
        Lineage::Covered {
            epoch,
            operation_id,
        } => (epoch, operation_id),
    };
    if claim.claims_role_authority() && (claim.provider.is_none() || claim.session_id.is_none()) {
        return RecoveryVerdict::Stripped(RecoveryRefusal::UnbackedRoleAuthority);
    }
    if let Some(reason) = journal_tail_refusal {
        return RecoveryVerdict::Stripped(RecoveryRefusal::JournalTailRefused {
            reason: reason.to_string(),
        });
    }
    let Some(checkpoint) = checkpoint else {
        return RecoveryVerdict::Stripped(RecoveryRefusal::NoCommittedCheckpoint);
    };
    if checkpoint.key.tab_id != claim.tab_id {
        return RecoveryVerdict::Stripped(RecoveryRefusal::NoCommittedCheckpoint);
    }
    if epoch != checkpoint.key.epoch {
        return RecoveryVerdict::Stripped(RecoveryRefusal::EpochMismatch {
            snapshot: epoch,
            checkpoint: checkpoint.key.epoch,
        });
    }
    if operation_id != checkpoint.key.operation_id {
        return RecoveryVerdict::Stripped(RecoveryRefusal::OperationMismatch {
            snapshot: operation_id.to_string(),
            checkpoint: checkpoint.key.operation_id.clone(),
        });
    }
    if let Some(field) = disagreeing_field(claim, &checkpoint.authority) {
        return RecoveryVerdict::Stripped(RecoveryRefusal::FieldMismatch { field });
    }
    RecoveryVerdict::Restored(Box::new(checkpoint.authority.clone()))
}

/// A tab whose lifecycle predates the journal keeps only what the prior
/// contract allowed: role authority still requires a provider identity.
fn pre_journal_verdict(claim: &SnapshotAuthorityClaim) -> RecoveryVerdict {
    if claim.claims_role_authority() && claim.provider.is_none() && !claim.chat_backend {
        return RecoveryVerdict::Stripped(RecoveryRefusal::UnbackedRoleAuthority);
    }
    RecoveryVerdict::PreJournal
}

/// A snapshot may hold less than the checkpoint, never more or different.
fn disagreeing_field(
    claim: &SnapshotAuthorityClaim,
    authority: &CommittedAuthority,
) -> Option<&'static str> {
    if !matches_or_absent(claim.provider.as_deref(), Some(authority.provider.as_str())) {
        return Some("provider");
    }
    if !matches_or_absent(
        claim.session_id.as_deref(),
        Some(authority.session_id.as_str()),
    ) {
        return Some("agent_session_id");
    }
    if !matches_or_absent(
        claim.binding_nonce.as_deref(),
        Some(authority.binding_nonce.as_str()),
    ) {
        return Some("session_binding_nonce");
    }
    if !matches_or_absent(claim.role.as_deref(), authority.role.as_deref()) {
        return Some("role");
    }
    if !matches_or_absent(
        claim.role_profile.as_deref(),
        authority.role_profile.as_deref(),
    ) {
        return Some("role_profile");
    }
    if !matches_or_absent(claim.team_id.as_deref(), authority.team_id.as_deref()) {
        return Some("team_id");
    }
    if !matches_or_absent(
        claim.parent_tab_id.as_deref(),
        authority.parent_tab_id.as_deref(),
    ) {
        return Some("parent_tab_id");
    }
    if claim.can_spawn && !authority.can_spawn {
        return Some("can_spawn");
    }
    if claim.depth > 0 && claim.depth != authority.depth {
        return Some("depth");
    }
    None
}

fn matches_or_absent(claimed: Option<&str>, committed: Option<&str>) -> bool {
    match claimed {
        None => true,
        Some(value) => committed == Some(value),
    }
}
