//! Each store consumer that used to scan every plan now asks SQL for the plans it
//! matches; these tests prove a non-matching plan is never loaded.

use super::load_audit::{assert_never_loaded, attempt, execution_plan, seed, start, task};
use super::mutations::agent_lifecycle::{AgentLifecyclePlanChange, VerifiedWorkerTransferBinding};
use super::task_queries::TaskListFilter;
use super::{paths, PlanStore};
use crate::plan::{CanvasElement, EffortLink, LoopSpec, PlanStatus, Section, SpecLink, TaskStatus};
use std::path::PathBuf;

fn fixture() -> (tempfile::TempDir, PlanStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let project = paths::project_root(dir.path()).display().to_string();
    (dir, store, project)
}

fn other_project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = paths::project_root(dir.path());
    (dir, root)
}

fn section(id: &str) -> Section {
    serde_json::from_value(serde_json::json!({ "id": id, "title": id, "body": "" }))
        .expect("section fixture")
}

fn running_loop(id: &str) -> LoopSpec {
    serde_json::from_value(serde_json::json!({
        "id": id, "target": "ship it", "budget": 3, "stop_when": null, "status": "running"
    }))
    .expect("loop fixture")
}

fn spec_link(spec_id: &str) -> SpecLink {
    SpecLink {
        spec_id: spec_id.into(),
        spec_section_id: "spec_sec".into(),
        plan_section_id: "plan_sec".into(),
    }
}

#[test]
fn spec_coverage_report_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let spec = store
        .create_spec("Spec".into(), project.clone(), 1)
        .unwrap();
    let mut linked = execution_plan(project.as_ref(), "linked", "owner", vec![]);
    linked.spec_links.push(spec_link(&spec.id));
    seed(&store, linked);
    let other = seed(
        &store,
        execution_plan(project.as_ref(), "other", "owner", vec![]),
    );
    start(&store);

    assert!(store.spec_coverage_report(&spec.id).is_some());
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn list_spec_links_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut linked = execution_plan(project.as_ref(), "linked", "owner", vec![]);
    linked.spec_links.push(spec_link("sp_audit"));
    let linked = seed(&store, linked);
    let other = seed(
        &store,
        execution_plan(project.as_ref(), "other", "owner", vec![]),
    );
    start(&store);

    let links = store.list_spec_links("sp_audit");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].0, linked);
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn delete_spec_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let spec = store
        .create_spec("Spec".into(), project.clone(), 1)
        .unwrap();
    let mut referencing = execution_plan(project.as_ref(), "referencing", "owner", vec![]);
    referencing.spec_refs.push(spec.id.clone());
    let referencing = seed(&store, referencing);
    let other = seed(
        &store,
        execution_plan(project.as_ref(), "other", "owner", vec![]),
    );
    start(&store);

    store.delete_spec(&spec.id, 5).unwrap();
    assert!(store.get(&referencing).unwrap().spec_refs.is_empty());
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn list_tasks_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let (_other_dir, other_root) = other_project();
    let here = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "here",
            "owner",
            vec![task("task_here", TaskStatus::Pending, None)],
        ),
    );
    let elsewhere = seed(
        &store,
        execution_plan(
            &other_root,
            "elsewhere",
            "owner",
            vec![task("task_far", TaskStatus::Pending, None)],
        ),
    );
    start(&store);

    let tasks = store.list_tasks(TaskListFilter {
        project_path: Some(project.clone()),
        ..Default::default()
    });
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].plan_id, here);
    assert_never_loaded(&store, &[&elsewhere]);
}

#[test]
fn show_task_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let holder = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "holder",
            "owner",
            vec![task("task_shown", TaskStatus::Pending, None)],
        ),
    );
    let other = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "other",
            "owner",
            vec![task("task_hidden", TaskStatus::Pending, None)],
        ),
    );
    start(&store);

    assert_eq!(store.show_task("task_shown").unwrap().plan_id, holder);
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn set_task_assignment_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let holder = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "holder",
            "owner",
            vec![task("task_assign", TaskStatus::Pending, Some("w1"))],
        ),
    );
    let other = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "other",
            "owner",
            vec![task("task_other", TaskStatus::Pending, None)],
        ),
    );
    start(&store);

    assert_eq!(
        store
            .set_task_assignment("task_assign", None, None, 5)
            .unwrap(),
        holder
    );
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn task_key_high_water_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut keyed = task("task_keyed", TaskStatus::Pending, None);
    keyed.key = Some("AUD-7".into());
    let mut lower = task("task_lower", TaskStatus::Done, None);
    lower.key = Some("AUD-3".into());
    seed(
        &store,
        execution_plan(project.as_ref(), "keyed", "owner", vec![keyed]),
    );
    seed(
        &store,
        execution_plan(project.as_ref(), "lower", "owner", vec![lower]),
    );
    start(&store);

    assert_eq!(store.task_key_high_water().get("AUD"), Some(&7));
    assert!(
        store.plan_loads_for_test().is_empty(),
        "keys come from the task rows"
    );
}

#[test]
fn orphan_attachment_dirs_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let (_other_dir, other_root) = other_project();
    let here = seed(
        &store,
        execution_plan(project.as_ref(), "here", "owner", vec![]),
    );
    let elsewhere = seed(
        &store,
        execution_plan(&other_root, "elsewhere", "owner", vec![]),
    );
    start(&store);

    assert!(store.orphan_attachment_dirs(project.as_ref()).is_empty());
    assert_eq!(store.plan_loads_for_test(), vec![here]);
    assert_never_loaded(&store, &[&elsewhere]);
}

#[test]
fn mirror_plan_index_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let (_other_dir, other_root) = other_project();
    let here = seed(
        &store,
        execution_plan(project.as_ref(), "here", "owner", vec![]),
    );
    let elsewhere = seed(
        &store,
        execution_plan(&other_root, "elsewhere", "owner", vec![]),
    );
    let plan = store.get(&here).unwrap();
    start(&store);

    store.mirror_plan_index(&plan).unwrap();
    assert_never_loaded(&store, &[&elsewhere]);
}

#[test]
fn reindex_project_index_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let (_other_dir, other_root) = other_project();
    let here = seed(
        &store,
        execution_plan(project.as_ref(), "here", "owner", vec![]),
    );
    let elsewhere = seed(
        &store,
        execution_plan(&other_root, "elsewhere", "owner", vec![]),
    );
    start(&store);

    store.reindex_project_index(project.as_ref()).unwrap();
    assert_eq!(store.plan_loads_for_test(), vec![here]);
    assert_never_loaded(&store, &[&elsewhere]);
}

#[test]
fn seed_missing_search_indexes_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let (_other_dir, other_root) = other_project();
    let missing = seed(
        &store,
        execution_plan(project.as_ref(), "missing", "owner", vec![]),
    );
    let indexed = seed(
        &store,
        execution_plan(&other_root, "indexed", "owner", vec![]),
    );
    let index = super::search_index::project_index_path(&other_root);
    std::fs::create_dir_all(index.parent().unwrap()).unwrap();
    std::fs::write(&index, b"").unwrap();
    start(&store);

    store.seed_missing_search_indexes();
    assert!(store.plan_loads_for_test().iter().all(|id| *id == missing));
    assert!(
        store.cached_plan_ids_for_test().is_empty(),
        "seeding never fills the cache"
    );
    assert_never_loaded(&store, &[&indexed]);
}

#[test]
fn related_units_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let ancestor = seed(
        &store,
        execution_plan(project.as_ref(), "ancestor", "owner", vec![]),
    );
    let mut holder = execution_plan(
        project.as_ref(),
        "holder",
        "owner",
        vec![task("task_related", TaskStatus::Pending, None)],
    );
    holder.lineage.push(EffortLink {
        to: ancestor.clone(),
        rel: crate::plan::EffortLinkRel::Follows,
    });
    seed(&store, holder);
    let other = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "other",
            "owner",
            vec![task("task_unrelated", TaskStatus::Pending, None)],
        ),
    );
    start(&store);

    let groups = store.related_units("task_related");
    let lineage = groups
        .iter()
        .find(|group| group.edge_type == "lineage")
        .unwrap();
    assert_eq!(lineage.units[0].id, ancestor);
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn resolve_loop_id_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut looping = execution_plan(project.as_ref(), "looping", "owner", vec![]);
    looping.loops.push(running_loop("loop_audit"));
    seed(&store, looping);
    seed(
        &store,
        execution_plan(project.as_ref(), "other", "owner", vec![]),
    );
    start(&store);

    assert_eq!(store.resolve_loop_id("loop_audit").unwrap(), "loop_audit");
    assert!(
        store.plan_loads_for_test().is_empty(),
        "loops come from the header rows"
    );
}

#[test]
fn loop_status_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut looping = execution_plan(project.as_ref(), "looping", "owner", vec![]);
    looping.loops.push(running_loop("loop_status"));
    seed(&store, looping);
    let other = seed(
        &store,
        execution_plan(project.as_ref(), "other", "owner", vec![]),
    );
    start(&store);

    assert_eq!(store.loop_status("loop_status").unwrap().id, "loop_status");
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn resolve_task_ref_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut keyed = task("task_ref", TaskStatus::Pending, None);
    keyed.key = Some("REF-4".into());
    let holder = seed(
        &store,
        execution_plan(project.as_ref(), "holder", "owner", vec![keyed]),
    );
    seed(
        &store,
        execution_plan(
            project.as_ref(),
            "other",
            "owner",
            vec![task("task_else", TaskStatus::Pending, None)],
        ),
    );
    start(&store);

    let resolved = store.resolve_task_ref(None, "ref-4").unwrap();
    assert_eq!(
        (resolved.plan_id, resolved.task_id),
        (holder, "task_ref".to_string())
    );
    assert!(
        store.plan_loads_for_test().is_empty(),
        "references come from the task rows"
    );
}

#[test]
fn resolve_section_ref_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut holder = execution_plan(project.as_ref(), "holder", "owner", vec![]);
    let mut parent = section("sec_parent");
    parent.subsections.push(section("sec_nested"));
    holder.sections.push(parent);
    let holder = seed(&store, holder);
    let mut other = execution_plan(project.as_ref(), "other", "owner", vec![]);
    other.sections.push(section("sec_other"));
    seed(&store, other);
    start(&store);

    let resolved = store.resolve_section_ref(None, "sec_nested").unwrap();
    assert_eq!(
        (resolved.plan_id, resolved.section_id),
        (holder, "sec_nested".to_string())
    );
    assert!(
        store.plan_loads_for_test().is_empty(),
        "sections come from the section rows"
    );
}

#[test]
fn relocate_workspace_projects_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    seed(
        &store,
        execution_plan(project.as_ref(), "bound", "moving-tab", vec![]),
    );
    let other = seed(
        &store,
        execution_plan(project.as_ref(), "other", "staying-tab", vec![]),
    );
    start(&store);

    let (updated, warnings) = store
        .relocate_workspace_projects(&["moving-tab".into()], &[], |path| Some(path.to_string()))
        .unwrap();
    assert!(updated.is_empty() && warnings.is_empty());
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn resolve_active_plan_for_orchestrator_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let owned = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "owned",
            "orch-a",
            vec![task("task_open", TaskStatus::Pending, None)],
        ),
    );
    let other = seed(
        &store,
        execution_plan(project.as_ref(), "other", "orch-b", vec![]),
    );
    start(&store);

    assert_eq!(
        store
            .resolve_active_plan_for_orchestrator(None, Some("orch-a"), None)
            .unwrap(),
        owned
    );
    assert_never_loaded(&store, &[&other]);
}

fn handoff(tab_id: &str) -> CanvasElement {
    serde_json::from_value(serde_json::json!({
        "id": format!("el_{tab_id}"),
        "kind": "handoff",
        "content": {
            "reseed_state": "new_sent",
            "reseed_pending": { "tab_id": tab_id, "source_session_id": "old-session" }
        }
    }))
    .expect("handoff fixture")
}

#[test]
fn observe_handoff_session_start_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut pending = execution_plan(project.as_ref(), "pending", "owner", vec![]);
    pending.elements.push(handoff("tab-reseed"));
    let pending = seed(&store, pending);
    let mut other = execution_plan(project.as_ref(), "other", "owner", vec![]);
    other.elements.push(handoff("tab-else"));
    let other = seed(&store, other);
    start(&store);

    store
        .observe_handoff_session_start("tab-reseed", "new-session", 5)
        .unwrap();
    assert!(store.plan_loads_for_test().contains(&pending));
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn reminder_candidate_plans_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let working = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "working",
            "owner",
            vec![task("task_w", TaskStatus::InProgress, Some("w1"))],
        ),
    );
    let mut archived = execution_plan(
        project.as_ref(),
        "archived",
        "owner",
        vec![task("task_a", TaskStatus::InProgress, Some("w1"))],
    );
    archived.status = PlanStatus::Archived;
    let archived = seed(&store, archived);
    let finished = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "finished",
            "owner",
            vec![task("task_f", TaskStatus::Done, None)],
        ),
    );
    start(&store);

    let ids: Vec<_> = store
        .reminder_candidate_plans()
        .into_iter()
        .map(|plan| plan.id)
        .collect();
    assert_eq!(ids, vec![working.clone()]);
    let for_tab: Vec<_> = store
        .reminder_candidate_plans_for_tab("w1")
        .into_iter()
        .map(|plan| plan.id)
        .collect();
    assert_eq!(for_tab, vec![working]);
    assert_never_loaded(&store, &[&archived, &finished]);
}

#[test]
fn close_team_task_attempts_transaction_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut team_plan = execution_plan(project.as_ref(), "team", "owner", vec![]);
    team_plan.team_id = Some("team-closing".into());
    let team_plan = seed(&store, team_plan);
    let mut member_work = task("task_member", TaskStatus::InProgress, None);
    member_work
        .execution_attempts
        .push(attempt("att_member", "member-tab"));
    let member_plan = seed(
        &store,
        execution_plan(project.as_ref(), "member", "owner", vec![member_work]),
    );
    let mut unrelated = execution_plan(project.as_ref(), "unrelated", "owner", vec![]);
    unrelated.team_id = Some("team-staying".into());
    let unrelated = seed(&store, unrelated);
    start(&store);

    let transaction = store
        .close_team_task_attempts_transaction("team-closing", &["member-tab".into()], 5)
        .unwrap();
    let mut changed = transaction.changed_plan_ids().to_vec();
    changed.sort();
    let mut expected = vec![team_plan, member_plan];
    expected.sort();
    assert_eq!(changed, expected);
    assert_never_loaded(&store, &[&unrelated]);
}

#[test]
fn apply_agent_lifecycle_changes_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let assigned = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "assigned",
            "owner",
            vec![task("task_gone", TaskStatus::InProgress, Some("gone-tab"))],
        ),
    );
    let other = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "other",
            "owner",
            vec![task("task_kept", TaskStatus::InProgress, Some("kept-tab"))],
        ),
    );
    start(&store);

    let transaction = store
        .apply_agent_lifecycle_changes(
            vec![AgentLifecyclePlanChange {
                tab_id: "gone-tab".into(),
                preserve_owned_plans: false,
                retain_owned_plan_for_team_recovery: None,
                preserve_task_bindings_for_owner: None,
                planner_binding: None,
                plan_rebind: None,
            }],
            5,
        )
        .unwrap();
    assert_eq!(transaction.changed_plan_ids(), [assigned]);
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn with_verified_worker_transfer_binding_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut bound = task("task_bound", TaskStatus::InProgress, Some("worker-tab"));
    bound
        .execution_attempts
        .push(attempt("att_bound", "worker-tab"));
    let holder = seed(
        &store,
        execution_plan(project.as_ref(), "holder", "owner", vec![bound]),
    );
    let other = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "other",
            "owner",
            vec![task("task_else", TaskStatus::InProgress, Some("else-tab"))],
        ),
    );
    start(&store);

    let expected = VerifiedWorkerTransferBinding {
        plan_id: holder,
        team_id: "team".into(),
        owner_tab_id: "owner".into(),
        owner_session_id: "owner-session".into(),
        task_id: "task_bound".into(),
        attempt_id: "att_bound".into(),
        worker_tab_id: "worker-tab".into(),
        workspace_id: "workspace".into(),
        worktree_path: "/lanes/worker".into(),
        session_id: "worker-session".into(),
        binding_nonce: "nonce".into(),
        provider: "fixture-provider-a".into(),
        owner_epoch: 1,
    };
    let mut committed = false;
    let refused = store.with_verified_worker_transfer_binding(&expected, || committed = true);
    assert!(
        refused.is_err() && !committed,
        "a lane-less binding is not unique"
    );
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn plan_id_for_team_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut bound = execution_plan(project.as_ref(), "bound", "owner", vec![]);
    bound.team_id = Some("team-bound".into());
    let bound = seed(&store, bound);
    seed(
        &store,
        execution_plan(project.as_ref(), "other", "owner", vec![]),
    );
    start(&store);

    assert_eq!(store.plan_id_for_team("team-bound"), Some(bound));
    assert!(
        store.plan_loads_for_test().is_empty(),
        "the index names the Team's plan"
    );
}

#[test]
fn for_each_plan_uncached_streams_every_plan_without_filling_the_cache() {
    let (_dir, store, project) = fixture();
    let first = seed(
        &store,
        execution_plan(project.as_ref(), "first", "owner", vec![]),
    );
    let second = seed(
        &store,
        execution_plan(project.as_ref(), "second", "owner", vec![]),
    );
    start(&store);

    let mut seen = Vec::new();
    store.for_each_plan_uncached(|plan| seen.push(plan.id));
    seen.sort();
    let mut expected = vec![first, second];
    expected.sort();
    assert_eq!(seen, expected);
    assert!(store.cached_plan_ids_for_test().is_empty());
}

#[test]
fn plans_owned_or_planned_by_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let owned = seed(
        &store,
        execution_plan(project.as_ref(), "owned", "tab-a", vec![]),
    );
    let mut planned = execution_plan(project.as_ref(), "planned", "owner", vec![]);
    planned.planner = super::load_audit::planner("tab-a");
    let planned = seed(&store, planned);
    let other = seed(
        &store,
        execution_plan(project.as_ref(), "other", "tab-b", vec![]),
    );
    start(&store);

    let mut ids: Vec<_> = store
        .plans_owned_or_planned_by("tab-a")
        .into_iter()
        .map(|plan| plan.id)
        .collect();
    ids.sort();
    let mut expected = vec![owned, planned];
    expected.sort();
    assert_eq!(ids, expected);
    assert_never_loaded(&store, &[&other]);
}

#[test]
fn team_close_plans_never_loads_unmatched_plans() {
    let (_dir, store, project) = fixture();
    let mut laned = task("task_lane", TaskStatus::Pending, None);
    laned.lane.tab_id = Some("member-tab".into());
    let laned = seed(
        &store,
        execution_plan(project.as_ref(), "laned", "owner", vec![laned]),
    );
    let assigned = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "assigned",
            "owner",
            vec![task("task_as", TaskStatus::Done, Some("member-tab"))],
        ),
    );
    let other = seed(
        &store,
        execution_plan(
            project.as_ref(),
            "other",
            "owner",
            vec![task("task_x", TaskStatus::InProgress, Some("outsider"))],
        ),
    );
    start(&store);

    let mut ids: Vec<_> = store
        .team_close_plans("team-closing", &["member-tab".into()])
        .into_iter()
        .map(|plan| plan.id)
        .collect();
    ids.sort();
    let mut expected = vec![laned, assigned];
    expected.sort();
    assert_eq!(ids, expected);
    assert_never_loaded(&store, &[&other]);
}
