use std::collections::HashMap;

use crate::session::store::{SessionSnapshot, SplitNodeSnapshot};

use super::journal::{JournalLoad, TailStatus};
use super::record::{LifecycleCheckpoint, LifecycleEpoch};
use super::recovery::{authorize, RecoveryRefusal, RecoveryVerdict, SnapshotAuthorityClaim};

/// What the gate decided for one tab, kept for the forensic log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabRecoveryOutcome {
    pub tab_id: String,
    pub verdict: RecoveryVerdict,
}

/// How the journal relates to the snapshot being restored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotCoverage {
    /// Written before coverage existed. Nothing is examined or stripped.
    PreCoverage,
    /// Written under the journal's current coverage. Enforced exactly.
    Active,
    /// Written under a coverage the journal no longer carries. The proof is
    /// gone, so every authority claim fails closed.
    Lost { snapshot_coverage: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotGate {
    pub coverage: SnapshotCoverage,
    pub tabs: Vec<TabRecoveryOutcome>,
}

/// Reduce a snapshot to the authority its journal proves, stripping the rest in
/// place. Only a snapshot written before coverage existed is left untouched.
pub fn gate_session_snapshot(
    snapshot: &mut SessionSnapshot,
    load: &JournalLoad,
    active_coverage: Option<&str>,
) -> SnapshotGate {
    let coverage = match (snapshot.lifecycle_coverage.clone(), active_coverage) {
        (None, _) => SnapshotCoverage::PreCoverage,
        (Some(stamp), Some(active)) if stamp == active => SnapshotCoverage::Active,
        (Some(stamp), _) => SnapshotCoverage::Lost {
            snapshot_coverage: stamp,
        },
    };
    if coverage == SnapshotCoverage::PreCoverage {
        return SnapshotGate {
            coverage,
            tabs: Vec::new(),
        };
    }
    let checkpoints: HashMap<String, LifecycleCheckpoint> = super::journal::checkpoints(load)
        .into_iter()
        .map(|checkpoint| (checkpoint.key.tab_id.clone(), checkpoint))
        .collect();
    let tail_refusal = match &load.tail {
        TailStatus::Intact => None,
        TailStatus::Refused { line, reason } => Some(format!("line {line}: {reason}")),
    };

    let mut outcomes = Vec::new();
    for workspace in &mut snapshot.workspaces {
        for tab in &mut workspace.tabs {
            visit_leaves(&mut tab.split_tree, &mut |leaf| {
                let claim = claim_from_leaf(leaf);
                let verdict = match &coverage {
                    SnapshotCoverage::Lost { snapshot_coverage } => {
                        coverage_lost_verdict(&claim, snapshot_coverage, active_coverage)
                    }
                    _ => authorize(
                        &claim,
                        checkpoints.get(&claim.tab_id),
                        tail_refusal.as_deref(),
                    ),
                };
                if verdict.is_stripped() {
                    strip_authority(leaf);
                }
                outcomes.push(TabRecoveryOutcome {
                    tab_id: claim.tab_id,
                    verdict,
                });
            });
        }
    }
    SnapshotGate {
        coverage,
        tabs: outcomes,
    }
}

fn coverage_lost_verdict(
    claim: &SnapshotAuthorityClaim,
    snapshot_coverage: &str,
    active_coverage: Option<&str>,
) -> RecoveryVerdict {
    if !claim.claims_authority() {
        return RecoveryVerdict::NoAuthorityClaimed;
    }
    RecoveryVerdict::Stripped(RecoveryRefusal::CoverageLost {
        snapshot: snapshot_coverage.to_string(),
        active: active_coverage.map(str::to_string),
    })
}

fn visit_leaves(node: &mut SplitNodeSnapshot, visit: &mut impl FnMut(&mut SplitNodeSnapshot)) {
    if let SplitNodeSnapshot::Split { children, .. } = node {
        for child in children {
            visit_leaves(child, visit);
        }
        return;
    }
    visit(node);
}

fn claim_from_leaf(leaf: &SplitNodeSnapshot) -> SnapshotAuthorityClaim {
    let SplitNodeSnapshot::Leaf {
        tab_id,
        agent_type,
        claude_session_id,
        agent_identity,
        role,
        role_profile,
        team_id,
        parent_tab_id,
        can_spawn,
        depth,
        ..
    } = leaf
    else {
        return SnapshotAuthorityClaim::default();
    };
    SnapshotAuthorityClaim {
        tab_id: tab_id.clone(),
        epoch: agent_identity
            .as_ref()
            .and_then(|identity| identity.lifecycle_epoch)
            .map(LifecycleEpoch),
        operation_id: agent_identity
            .as_ref()
            .and_then(|identity| identity.lifecycle_operation_id.clone()),
        provider: agent_type.clone(),
        session_id: claude_session_id.clone(),
        binding_nonce: agent_identity
            .as_ref()
            .and_then(|identity| identity.reported_session_binding_nonce.clone()),
        role: role.clone(),
        role_profile: role_profile.clone(),
        team_id: team_id.clone(),
        parent_tab_id: parent_tab_id.clone(),
        can_spawn: *can_spawn,
        depth: *depth,
    }
}

/// Drop every authority field. Presentation and the tab id survive, so the tab
/// comes back as a plain shell.
fn strip_authority(leaf: &mut SplitNodeSnapshot) {
    let SplitNodeSnapshot::Leaf {
        agent_type,
        agent_type_locked,
        claude_session_id,
        agent_identity,
        role,
        role_profile,
        team_id,
        parent_tab_id,
        can_spawn,
        depth,
        chatbackend_plugin_id,
        chatbackend_session_id,
        ..
    } = leaf
    else {
        return;
    };
    *agent_type = None;
    *agent_type_locked = false;
    *claude_session_id = None;
    *role = None;
    *role_profile = None;
    *team_id = None;
    *parent_tab_id = None;
    *can_spawn = false;
    *depth = 0;
    *chatbackend_plugin_id = None;
    *chatbackend_session_id = None;
    if let Some(identity) = agent_identity {
        identity.session_binding_nonce = None;
        identity.reported_session_binding_nonce = None;
        identity.agent_state = None;
        identity.boot_stage = None;
        identity.lifecycle_epoch = None;
        identity.lifecycle_operation_id = None;
    }
}
