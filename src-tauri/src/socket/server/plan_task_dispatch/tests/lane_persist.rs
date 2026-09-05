//! A dispatch onto a PRE-CREATED worktree stores the lane it reports. The
//! assertions compare the stored lane against the emitted `dispatch_ready`
//! block, never against literals: the defect was the two disagreeing, so a test
//! that pins both to the same hand-written string could not have caught it.

use super::super::{dispatch_ready_event, lane_metadata_for_dispatch, resolve_dispatch_lane};
use crate::plan::TaskLaneMetadata;
use crate::service::LaneProtection;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(path: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    crate::process_util::no_window_std(&mut command);
    let output = command.current_dir(path).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn project(container: &Path) -> PathBuf {
    let project = container.join("project");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "Test"]);
    std::fs::write(project.join("README.md"), "fixture").unwrap();
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-qm", "fixture"]);
    project
}

/// The shape the Orchestrator hands `plan task dispatch --cwd`: a worktree that
/// already exists, on its own branch, that the dispatch did not provision.
fn pre_created_worktree(project: &Path, container: &Path) -> PathBuf {
    let lane = container.join("lanes").join("pre-created");
    std::fs::create_dir_all(lane.parent().unwrap()).unwrap();
    git(
        project,
        &[
            "worktree",
            "add",
            "-b",
            "lane/pre-created",
            lane.to_str().unwrap(),
        ],
    );
    lane
}

fn emitted_lane(event: &Value) -> &Value {
    event
        .get("lane")
        .expect("dispatch_ready must carry the lane it resolved")
}

#[test]
fn dispatch_onto_a_pre_created_worktree_stores_the_lane_it_reports() {
    let container = tempfile::tempdir().unwrap();
    let project = project(container.path());
    let lane_path = pre_created_worktree(&project, container.path());

    let resolved = resolve_dispatch_lane(
        Some(lane_path.to_str().unwrap()),
        Some(project.to_str().unwrap()),
        None,
        // The task carries NO recorded lane: this is its first dispatch, which
        // is exactly the case that used to drop what it reported.
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap();
    let provenance = resolved
        .existing
        .expect("a pre-created worktree resolves as an existing lane");

    let event = dispatch_ready_event(
        "pl_test",
        "task_test",
        "tab-worker",
        "attempt-1",
        &super::super::DispatchEvidence {
            session_id: "session-worker".into(),
            lane: Some(provenance.clone()),
        },
        project.to_str().unwrap(),
        &[],
    );
    let reported = emitted_lane(&event);

    // The same transition's committed value.
    let stored = lane_metadata_for_dispatch(
        &TaskLaneMetadata::default(),
        &provenance,
        project.to_str().unwrap(),
    );

    assert_eq!(
        stored.branch.as_deref(),
        reported.get("branch").and_then(Value::as_str),
        "stored lane.branch must equal the branch dispatch_ready reported"
    );
    assert_eq!(
        stored.worktree_path.as_deref(),
        reported.get("worktree_path").and_then(Value::as_str),
        "stored lane.worktree_path must equal the path dispatch_ready reported"
    );
    assert_eq!(
        stored.base.as_deref(),
        reported.get("base").and_then(Value::as_str),
        "stored lane.base must equal the base dispatch_ready reported"
    );
    assert!(
        stored.worktree_path.is_some() && stored.branch.is_some() && stored.base.is_some(),
        "the equality above must not be satisfied by both sides being empty"
    );

    git(
        &project,
        &["worktree", "remove", "--force", lane_path.to_str().unwrap()],
    );
}

/// The companion refusal. `report_guard` refuses a readiness report whose task
/// requires a repository but resolves no lane worktree, and its remedy is a
/// re-dispatch — which repeated the hole. With the lane stored, the condition
/// that produced that refusal no longer holds.
#[test]
fn a_stored_lane_leaves_no_unresolved_worktree_for_in_review() {
    let container = tempfile::tempdir().unwrap();
    let project = project(container.path());
    let lane_path = pre_created_worktree(&project, container.path());

    let provenance = resolve_dispatch_lane(
        Some(lane_path.to_str().unwrap()),
        Some(project.to_str().unwrap()),
        None,
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap()
    .existing
    .unwrap();
    let stored = lane_metadata_for_dispatch(
        &TaskLaneMetadata::default(),
        &provenance,
        project.to_str().unwrap(),
    );

    // `report_lane::task_readiness_scope` reads exactly this field to decide
    // whether a lane worktree resolved; `None` is what triggered the refusal.
    assert!(
        stored.worktree_path.is_some(),
        "in_review was refused because this was None after a dispatch that reported a lane"
    );

    git(
        &project,
        &["worktree", "remove", "--force", lane_path.to_str().unwrap()],
    );
}

/// The behaviour the wipe exists for must survive: dispatching onto the plan's
/// own project checkout records no lane, because there is no isolation to record.
#[test]
fn dispatch_onto_the_canonical_project_records_no_lane() {
    let container = tempfile::tempdir().unwrap();
    let project = project(container.path());

    let provenance = resolve_dispatch_lane(
        Some(project.to_str().unwrap()),
        Some(project.to_str().unwrap()),
        None,
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap()
    .existing
    .unwrap();
    let stored = lane_metadata_for_dispatch(
        &TaskLaneMetadata::default(),
        &provenance,
        project.to_str().unwrap(),
    );

    assert!(stored.worktree_path.is_none());
    assert!(stored.branch.is_none());
    assert!(stored.base.is_none());
}
