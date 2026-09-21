//! What a dispatch COMMITS about the lane and what it REPORTS about it.
//! Kept in one file because the two disagreeing is the defect these exist to
//! prevent: `dispatch_ready` must never name a lane the commit dropped.

use serde_json::{json, Value};

use super::super::safety_nudge::{apply_safety_nudges, SafetyNudge};
use crate::plan::TaskLaneMetadata;
use crate::service::{same_path, WorkerLaneProvenance};

/// `canonical_project_path` is the PLAN's project checkout, and landing there is
/// the one case with no lane to record. It must be passed in: a lane resolved
/// from an existing worktree is verified against itself, so its provenance
/// carries the same path in `worktree_path` and `canonical_cwd` and comparing
/// those two silently classified every pre-created worktree as "no isolation" —
/// dropping a lane the dispatch had already reported (codeterm#425).
///
/// Both comparisons RESOLVE the paths rather than matching them as text. The
/// provenance is already resolved (`verify_worker_lane` canonicalizes it) while
/// the plan record and the task lane are stored as given, so a textual compare
/// asks the wrong question the moment the two spellings differ — which is every
/// macOS tempdir, where `/var` is a symlink to `/private/var`.
pub(super) fn lane_metadata_for_dispatch(
    original: &TaskLaneMetadata,
    provenance: &WorkerLaneProvenance,
    canonical_project_path: &str,
) -> TaskLaneMetadata {
    let mut lane = original.clone();
    let recorded_worktree = original
        .worktree_path
        .as_deref()
        .is_some_and(|recorded| same_path(recorded, &provenance.worktree_path));
    if !recorded_worktree && same_path(&provenance.worktree_path, canonical_project_path) {
        lane.worktree_path = None;
        lane.branch = None;
        lane.base = None;
        lane.cleanup_status = None;
        return lane;
    }
    lane.worktree_path = Some(provenance.worktree_path.clone());
    // A waived detached lane has no branch; record the absence so the advisory
    // nudge fires instead of persisting an empty branch name.
    lane.branch = (!provenance.branch.is_empty()).then(|| provenance.branch.clone());
    lane.base = Some(provenance.base.clone());
    lane.cleanup_status = None;
    lane
}

pub(super) fn clear_replaced_lane_routing_fields(lane: &mut TaskLaneMetadata) {
    lane.tab_id = None;
    lane.workspace_id = None;
    lane.workspace_name = None;
    lane.worktree_path = None;
    lane.branch = None;
}

#[derive(Clone, Debug)]
pub(super) struct DispatchEvidence {
    pub(super) session_id: String,
    pub(super) lane: Option<WorkerLaneProvenance>,
}

pub(super) fn dispatch_ready_event(
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    attempt_id: &str,
    evidence: &DispatchEvidence,
    canonical_project_path: &str,
    nudges: &[SafetyNudge],
) -> Value {
    let mut event = json!({
        "kind": "dispatch_ready",
        "plan_id": plan_id,
        "task_id": task_id,
        "tab_id": tab_id,
        "attempt_id": attempt_id,
        "session_id": evidence.session_id,
    });
    if let Some(lane) = evidence.lane.as_ref() {
        event["lane"] = json!({
            "worktree_path": lane.worktree_path,
            "branch": lane.branch,
            "base": lane.base,
            // The plan's project checkout, not the lane verified against itself.
            "canonical_project_path": canonical_project_path,
        });
    }
    apply_safety_nudges(&mut event, nudges);
    event
}

/// The immediate dispatch response's lane block. It lives beside
/// `dispatch_ready_event` because the two must name the same canonical
/// checkout: the response used the provenance's own `canonical_cwd`, which for
/// a lane verified against itself is the WORKER lane (codeterm#77).
pub(super) fn dispatch_response_lane(
    provenance: &WorkerLaneProvenance,
    canonical_project_path: &str,
    lane: &TaskLaneMetadata,
) -> Value {
    json!({
        "worktree_path": provenance.worktree_path,
        "branch": provenance.branch,
        "base": provenance.base,
        "canonical_project_path": canonical_project_path,
        "destination": lane.destination,
        "workspace_id": lane.workspace_id,
        "workspace_name": lane.workspace_name,
    })
}
