use std::collections::HashMap;

use crate::session::store::{SessionSnapshot, SplitNodeSnapshot};

use super::journal::{JournalLoad, TailStatus};
use super::record::{LifecycleCheckpoint, LifecycleEpoch};
use super::recovery::{authorize, RecoveryVerdict, SnapshotAuthorityClaim};

/// What the gate decided for one tab, kept for the forensic log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabRecoveryOutcome {
    pub tab_id: String,
    pub verdict: RecoveryVerdict,
}

/// Reduce a snapshot to the authority its journal proves, stripping the rest in
/// place. Each tab is judged on its own lineage, never on a global flag.
pub fn gate_session_snapshot(
    snapshot: &mut SessionSnapshot,
    load: &JournalLoad,
) -> Vec<TabRecoveryOutcome> {
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
                let verdict = authorize(
                    &claim,
                    checkpoints.get(&claim.tab_id),
                    tail_refusal.as_deref(),
                );
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
    outcomes
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
        chatbackend_plugin_id,
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
        chat_backend: chatbackend_plugin_id.is_some(),
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
        agent_session_model,
        agent_planned_model,
        agent_requested_model,
        agent_provider_resolved_model,
        model_fallback,
        model_identity_mismatch,
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
    *agent_session_model = None;
    *agent_planned_model = None;
    *agent_requested_model = None;
    *agent_provider_resolved_model = None;
    *model_fallback = None;
    *model_identity_mismatch = None;
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
