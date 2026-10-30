//! Tab and attempt binding queries run on hot paths; each must load only the plans
//! whose stored task or attempt rows name the tab.

use super::load_audit::{assert_never_loaded, attempt, execution_plan, planner, seed, start, task};
use super::task_attempts::CompleteAcknowledgedDispatchBinding;
use super::{paths, PlanStore};
use crate::plan::{PlanMode, TaskStatus};

fn fixture() -> (tempfile::TempDir, PlanStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path()).display().to_string();
    (dir, store, project)
}

/// One plan whose task is assigned to `tab_id` and one assigned elsewhere.
fn assigned_pair(
    store: &PlanStore,
    project: &str,
    tab_id: &str,
    status: TaskStatus,
) -> (String, String) {
    let matched = seed(
        store,
        execution_plan(
            project.as_ref(),
            "matched",
            "owner",
            vec![task("task_m", status.clone(), Some(tab_id))],
        ),
    );
    let unmatched = seed(
        store,
        execution_plan(
            project.as_ref(),
            "unmatched",
            "owner",
            vec![task("task_u", status, Some("other-tab"))],
        ),
    );
    (matched, unmatched)
}

/// One plan whose task the tab executes through an open attempt, and one executed elsewhere.
fn attempt_pair(store: &PlanStore, project: &str, tab_id: &str) -> (String, String) {
    let mut mine = task("task_m", TaskStatus::InProgress, Some(tab_id));
    let mut bound = attempt("att_m", tab_id);
    bound.session_id = Some("session-m".into());
    bound.session_binding_nonce = Some("nonce-m".into());
    mine.execution_attempts.push(bound);
    let mut theirs = task("task_u", TaskStatus::InProgress, Some("other-tab"));
    theirs
        .execution_attempts
        .push(attempt("att_u", "other-tab"));
    let matched = seed(
        store,
        execution_plan(project.as_ref(), "matched", "owner", vec![mine]),
    );
    let unmatched = seed(
        store,
        execution_plan(project.as_ref(), "unmatched", "owner", vec![theirs]),
    );
    (matched, unmatched)
}

#[test]
fn task_binding_for_tab_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let (matched, unmatched) = assigned_pair(&store, &project, "tab-a", TaskStatus::InProgress);
    start(&store);

    assert_eq!(
        store.task_binding_for_tab("tab-a"),
        Some((matched, "task_m".into()))
    );
    assert_never_loaded(&store, &[&unmatched]);
}

#[test]
fn open_attempt_task_for_tab_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let (matched, unmatched) = attempt_pair(&store, &project, "tab-a");
    start(&store);

    assert_eq!(
        store.open_attempt_task_for_tab("tab-a", "session-m", "nonce-m"),
        Some((matched, "task_m".into(), "att_m".into()))
    );
    assert_never_loaded(&store, &[&unmatched]);
}

#[test]
fn report_pending_task_binding_for_tab_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let (matched, unmatched) = assigned_pair(&store, &project, "tab-a", TaskStatus::Blocked);
    start(&store);

    assert_eq!(
        store.report_pending_task_binding_for_tab("tab-a"),
        Some((matched, "task_m".into()))
    );
    assert_never_loaded(&store, &[&unmatched]);
}

#[test]
fn active_owned_plan_with_open_work_for_tab_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let owned = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "owned",
            "orch-a",
            vec![task("task_o", TaskStatus::Pending, None)],
        ),
    );
    let other = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "other",
            "orch-b",
            vec![task("task_x", TaskStatus::Pending, None)],
        ),
    );
    start(&store);

    assert_eq!(
        store.active_owned_plan_with_open_work_for_tab("orch-a"),
        Some(owned)
    );
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn planner_plan_id_for_tab_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut planned = execution_plan(project.as_ref(), "planned", "owner", vec![]);
    planned.planner = planner("planner-a");
    let planned = seed(&store, planned);
    let mut other = execution_plan(project.as_ref(), "other", "owner", vec![]);
    other.planner = planner("planner-b");
    let other = seed(&store, other);
    start(&store);

    assert_eq!(store.planner_plan_id_for_tab("planner-a"), Some(planned));
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn taskless_plan_context_candidates_for_tab_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let owned = seed(
        &store,
        execution_plan(project.as_ref(), "owned", "tab-a", vec![]),
    );
    let mut planned = execution_plan(project.as_ref(), "planned", "owner", vec![]);
    planned.planner = planner("tab-a");
    let planned = seed(&store, planned);
    let other = seed(
        &store,
        execution_plan(project.as_ref(), "other", "tab-b", vec![]),
    );
    start(&store);

    let mut expected = vec![owned, planned];
    expected.sort();
    assert_eq!(
        store.taskless_plan_context_candidates_for_tab("tab-a"),
        expected
    );
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn task_bindings_for_tab_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let (matched, unmatched) = attempt_pair(&store, &project, "tab-a");
    start(&store);

    assert_eq!(
        store.task_bindings_for_tab("tab-a"),
        vec![(matched, "task_m".into())]
    );
    assert_never_loaded(&store, &[&unmatched]);
}

#[test]
fn task_bindings_with_name_for_tab_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let (matched, unmatched) = assigned_pair(&store, &project, "tab-a", TaskStatus::InProgress);
    start(&store);

    assert_eq!(
        store.task_bindings_with_name_for_tab("tab-a"),
        vec![(matched, "task_m".into(), "task_m".into())]
    );
    assert_never_loaded(&store, &[&unmatched]);
}

#[test]
fn live_task_attempts_for_tab_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let (matched, unmatched) = attempt_pair(&store, &project, "tab-a");
    start(&store);

    let live = store.live_task_attempts_for_tab("tab-a");
    assert_eq!(live.len(), 1);
    assert_eq!(
        (live[0].plan_id.as_str(), live[0].attempt.id.as_str()),
        (matched.as_str(), "att_m")
    );
    assert_never_loaded(&store, &[&unmatched]);
}

#[test]
fn superseded_attempt_for_tab_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut handed_over = task("task_m", TaskStatus::InProgress, Some("new-tab"));
    let mut old = attempt("att_old", "old-tab");
    old.closed_at = Some(2);
    handed_over.execution_attempts.push(old);
    handed_over
        .execution_attempts
        .push(attempt("att_new", "new-tab"));
    let matched = seed(
        &store,
        execution_plan(project.as_ref(), "matched", "owner", vec![handed_over]),
    );
    let mut other = task("task_u", TaskStatus::InProgress, Some("else-tab"));
    other.execution_attempts.push(attempt("att_u", "else-tab"));
    let unmatched = seed(
        &store,
        execution_plan(project.as_ref(), "unmatched", "owner", vec![other]),
    );
    start(&store);

    let superseded = store.superseded_attempt_for_tab("old-tab").unwrap();
    assert_eq!(superseded.0, matched);
    assert_never_loaded(&store, &[&unmatched]);
}

#[test]
fn mark_live_task_attempt_runtime_ready_for_tab_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut finished = task("task_done", TaskStatus::Done, Some("tab-a"));
    finished
        .execution_attempts
        .push(attempt("att_done", "tab-a"));
    let finished = seed(
        &store,
        execution_plan(project.as_ref(), "finished", "owner", vec![finished]),
    );
    let mut other = task("task_u", TaskStatus::InProgress, Some("other-tab"));
    other.execution_attempts.push(attempt("att_u", "other-tab"));
    let other = seed(
        &store,
        execution_plan(project.as_ref(), "other", "owner", vec![other]),
    );
    start(&store);

    let ready = store
        .mark_live_task_attempt_runtime_ready_for_tab("tab-a", "session-a", 5)
        .unwrap();
    assert!(ready.is_none(), "a finished task's attempt is not live");
    assert_never_loaded(&store, &[&finished, &other]);
}

#[test]
fn recent_worker_attempt_titles_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut mine = task("task_m", TaskStatus::InProgress, Some("tab-a"));
    mine.lane.workspace_id = Some("ws-a".into());
    let mut titled = attempt("att_m", "tab-a");
    titled.role = Some("worker".into());
    titled.executor_title = Some("Bolt".into());
    mine.execution_attempts.push(titled);
    seed(
        &store,
        execution_plan(project.as_ref(), "matched", "owner", vec![mine]),
    );
    let mut theirs = task("task_u", TaskStatus::InProgress, Some("tab-b"));
    theirs.lane.workspace_id = Some("ws-b".into());
    let unmatched = seed(
        &store,
        execution_plan(project.as_ref(), "unmatched", "owner", vec![theirs]),
    );
    start(&store);

    assert_eq!(
        store.recent_worker_attempt_titles("ws-a", None, 5),
        vec!["Bolt".to_string()]
    );
    assert_never_loaded(&store, &[&unmatched]);
}

#[test]
fn current_plan_skill_attempt_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let (matched, unmatched) = attempt_pair(&store, &project, "tab-a");
    start(&store);

    assert_eq!(
        store
            .current_plan_skill_attempt("tab-a", "session-m", "nonce-m")
            .unwrap(),
        Some((matched, "task_m".into(), "att_m".into()))
    );
    assert_never_loaded(&store, &[&unmatched]);
}

#[test]
fn complete_acknowledged_dispatch_binding_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut mine = task("task_m", TaskStatus::InProgress, Some("tab-a"));
    mine.execution_attempts.push(attempt("att_m", "tab-a"));
    let mut team_plan = execution_plan(project.as_ref(), "matched", "owner", vec![mine]);
    team_plan.team_id = Some("team-a".into());
    let matched = seed(&store, team_plan);
    let mut theirs = task("task_u", TaskStatus::InProgress, Some("other-tab"));
    theirs
        .execution_attempts
        .push(attempt("att_u", "other-tab"));
    let unmatched = seed(
        &store,
        execution_plan(project.as_ref(), "unmatched", "owner", vec![theirs]),
    );
    start(&store);

    let refused = store.complete_acknowledged_dispatch_binding(
        CompleteAcknowledgedDispatchBinding {
            plan_id: matched,
            task_id: "task_m".into(),
            attempt_id: "att_m".into(),
            tab_id: "tab-a".into(),
            team_id: "team-a".into(),
            creator_tab_id: "owner".into(),
            provider: "fixture-provider-a".into(),
            session_id: "session-a".into(),
            nonce: "nonce-a".into(),
            owner_epoch: 1,
        },
        5,
    );
    assert!(
        refused.is_err(),
        "a task without its lane binding is refused"
    );
    assert_never_loaded(&store, &[&unmatched]);
}

#[test]
fn planner_replay_candidates_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut planned = execution_plan(project.as_ref(), "planned", "owner", vec![]);
    planned.planner = planner("planner-a");
    let planned = seed(&store, planned);
    let mut other = execution_plan(project.as_ref(), "other", "owner", vec![]);
    other.planner = planner("planner-b");
    let other = seed(&store, other);
    start(&store);

    assert!(
        store.planner_replay_candidates("planner-a").is_empty(),
        "a planner with no open request has nothing to replay"
    );
    assert_eq!(store.plan_loads_for_test(), vec![planned]);
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn prepend_worker_charter_for_tab_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let (_matched, unmatched) = assigned_pair(&store, &project, "tab-a", TaskStatus::InProgress);
    let mut planning = execution_plan(
        project.as_ref(),
        "planning",
        "owner",
        vec![task("task_p", TaskStatus::InProgress, Some("tab-a"))],
    );
    planning.mode = PlanMode::Planning;
    let planning = seed(&store, planning);
    start(&store);

    let text = store.prepend_worker_charter_for_tab("tab-a", "nudge");
    assert!(text.len() > "nudge".len() && text.ends_with("nudge"));
    assert_never_loaded(&store, &[&unmatched, &planning]);
}
