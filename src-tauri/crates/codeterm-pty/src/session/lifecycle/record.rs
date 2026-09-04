use serde::{Deserialize, Serialize};

/// Monotonic position in the journal, and the only ordering authority.
pub type JournalSeq = u64;

/// Per-tab generation. A new epoch opens at every launch.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct LifecycleEpoch(pub u64);

impl LifecycleEpoch {
    pub const fn first() -> Self {
        Self(1)
    }

    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

impl std::fmt::Display for LifecycleEpoch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identity of exactly one tab lifecycle operation. Every authority claim is
/// proven against this triple; two of the three matching is not a match.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LifecycleKey {
    pub tab_id: String,
    pub epoch: LifecycleEpoch,
    pub operation_id: String,
}

impl LifecycleKey {
    pub fn new(
        tab_id: impl Into<String>,
        epoch: LifecycleEpoch,
        operation_id: impl Into<String>,
    ) -> Self {
        Self {
            tab_id: tab_id.into(),
            epoch,
            operation_id: operation_id.into(),
        }
    }
}

/// A committed transition, written by the owner of the mutation in the same
/// operation that performs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transition", rename_all = "snake_case")]
pub enum LifecycleTransition {
    /// A fresh lifecycle operation begins. Carries no authority by itself.
    EpochOpened,
    /// Provider identity accepted for this operation.
    ProviderIdentityCommitted {
        provider: String,
    },
    /// Provider session bound and proven by the echoed launch nonce.
    SessionBound {
        session_id: String,
        binding_nonce: String,
    },
    /// Role and profile as committed. `role: None` records a role that was
    /// deliberately cleared, which is a commit like any other.
    RoleBinding {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role_profile: Option<String>,
        can_spawn: bool,
        depth: u8,
    },
    TeamBinding {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_tab_id: Option<String>,
    },
    /// Seals the operation. Only a sealed operation yields a checkpoint.
    LaunchReady,
    RestartRequested,
    /// Authority withdrawn; the operation can never seal afterwards.
    RolledBack {
        reason: String,
    },
    /// Terminal. `reason` is a `TabCloseReason` label, so the journal can name
    /// which seam closed the tab; pre-attribution records read `unattributed`.
    Closed {
        #[serde(default = "unattributed_close_reason")]
        reason: String,
    },
}

fn unattributed_close_reason() -> String {
    "unattributed".to_string()
}

impl LifecycleTransition {
    pub fn label(&self) -> &'static str {
        match self {
            Self::EpochOpened => "epoch_opened",
            Self::ProviderIdentityCommitted { .. } => "provider_identity_committed",
            Self::SessionBound { .. } => "session_bound",
            Self::RoleBinding { .. } => "role_binding",
            Self::TeamBinding { .. } => "team_binding",
            Self::LaunchReady => "launch_ready",
            Self::RestartRequested => "restart_requested",
            Self::RolledBack { .. } => "rolled_back",
            Self::Closed { .. } => "closed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleEntry {
    pub seq: JournalSeq,
    pub key: LifecycleKey,
    pub transition: LifecycleTransition,
    /// Diagnostic UTC stamp for "what changed and when" reading. Never consulted
    /// for ordering, lineage, checkpoint selection, or conflict resolution.
    #[serde(default)]
    pub recorded_at: String,
}

/// Every authority field a restore is allowed to hand back. Anything not here
/// is presentation or runtime state and is never sourced from a checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedAuthority {
    pub provider: String,
    pub session_id: String,
    pub binding_nonce: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tab_id: Option<String>,
    #[serde(default)]
    pub can_spawn: bool,
    #[serde(default)]
    pub depth: u8,
}

/// A complete, sealed operation. Its existence is the only evidence that lets a
/// snapshot keep authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleCheckpoint {
    pub key: LifecycleKey,
    /// Journal position that sealed this operation.
    pub committed_through: JournalSeq,
    pub authority: CommittedAuthority,
    /// Diagnostic UTC stamp copied from the sealing transition. Never evidence.
    #[serde(default)]
    pub sealed_at: String,
}

/// A journal line. Compaction replaces transitions with the checkpoints they
/// proved, reaching the same checkpoints on replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case")]
pub enum JournalRecord {
    Transition { entry: LifecycleEntry },
    Checkpoint { checkpoint: LifecycleCheckpoint },
}

impl JournalRecord {
    pub fn seq(&self) -> JournalSeq {
        match self {
            Self::Transition { entry } => entry.seq,
            Self::Checkpoint { checkpoint } => checkpoint.committed_through,
        }
    }

    pub fn tab_id(&self) -> &str {
        match self {
            Self::Transition { entry } => &entry.key.tab_id,
            Self::Checkpoint { checkpoint } => &checkpoint.key.tab_id,
        }
    }
}
