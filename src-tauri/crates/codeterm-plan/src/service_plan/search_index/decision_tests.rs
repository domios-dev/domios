use super::*;
use crate::plan::{Section, Spec};

pub(in crate::service_plan::search_index) fn sample_plan() -> Plan {
    let mut plan = Plan::new(
        "Index decisions".into(),
        "/decision-project".into(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_decisions".into();
    plan.tasks = ["alpha", "beta", "gamma"]
        .into_iter()
        .map(crate::service_plan::test_support::task)
        .collect();
    plan.sections = ["one", "two"]
        .into_iter()
        .map(|id| {
            serde_json::from_value(serde_json::json!({"id":id,"title":id,"body":id})).unwrap()
        })
        .collect::<Vec<Section>>();
    plan
}

fn signatures(units: &[Unit]) -> HashMap<String, IndexedUnit> {
    units
        .iter()
        .map(|unit| (unit.id.clone(), unit_signature(unit)))
        .collect()
}

#[test]
fn changed_task_selects_only_its_embedding() {
    let mut plan = sample_plan();
    let existing = signatures(&plan_units(&plan, "scope"));
    plan.tasks[1].body = "new body".into();
    let changes = unit_changes(&existing, &plan_units(&plan, "scope"));
    assert_eq!(changes.row_changes.len(), 1);
    assert_eq!(
        changes
            .embedding_changes
            .iter()
            .map(|u| u.id.as_str())
            .collect::<Vec<_>>(),
        ["planunit_pl_decisions_task_beta"]
    );
    assert_eq!(
        changes.stale_vectors,
        HashSet::from(["planunit_pl_decisions_task_beta".into()])
    );
}

#[test]
fn changed_plan_section_selects_only_its_embedding() {
    let mut plan = sample_plan();
    let existing = signatures(&plan_units(&plan, "scope"));
    plan.sections[1].body = "changed section".into();
    let changes = unit_changes(&existing, &plan_units(&plan, "scope"));
    assert_eq!(changes.embedding_changes.len(), 1);
    assert_eq!(changes.embedding_changes[0].unit_id, "two");
}

#[test]
fn changed_spec_section_selects_only_its_embedding() {
    let mut spec = Spec::new("Spec".into(), "/decision-project".into());
    spec.sections = sample_plan().sections;
    let existing = signatures(&spec_units(&spec, "scope"));
    spec.sections[0].body = "changed spec".into();
    let changes = unit_changes(&existing, &spec_units(&spec, "scope"));
    assert_eq!(changes.embedding_changes.len(), 1);
    assert_eq!(changes.embedding_changes[0].unit_id, "one");
}

#[test]
fn deleted_units_and_replaced_text_invalidate_vectors() {
    let mut plan = sample_plan();
    let existing = signatures(&plan_units(&plan, "scope"));
    plan.tasks.remove(2);
    plan.tasks[0].body = "replacement".into();
    let changes = unit_changes(&existing, &plan_units(&plan, "scope"));
    assert_eq!(
        changes.stale_vectors,
        HashSet::from([
            "planunit_pl_decisions_task_gamma".into(),
            "planunit_pl_decisions_task_alpha".into()
        ])
    );
    assert_eq!(changes.row_changes.len(), 1);
    assert!(changes.row_changes[0].body.contains("replacement"));
    assert!(!changes
        .row_changes
        .iter()
        .any(|unit| unit.unit_id == "gamma"));
}

#[test]
fn metadata_changes_do_not_reembed_unchanged_text() {
    let mut plan = sample_plan();
    let existing = signatures(&plan_units(&plan, "scope"));
    plan.updated_at = 99;
    let changes = unit_changes(&existing, &plan_units(&plan, "scope"));
    assert!(!changes.row_changes.is_empty());
    assert!(changes.embedding_changes.is_empty());
    assert!(changes.stale_vectors.is_empty());
}

#[test]
fn archived_units_require_history_or_archived_search() {
    let mut plan = sample_plan();
    plan.status = PlanStatus::Archived;
    for unit in plan_units(&plan, "scope") {
        let hit = plan_hit_at(
            PlanIndexHit {
                score: 1.0,
                id: unit.id.clone(),
                kind: unit.kind.into(),
                scope: unit.scope.clone(),
                title: unit.title.clone(),
                body: unit.body.clone(),
                source: Some(unit_signature(&unit).source),
                updated_at: "0".into(),
            },
            100,
        );
        assert!(!plan_hit_matches_status(&hit, PlanSearchStatus::Active));
        assert!(plan_hit_matches_status(&hit, PlanSearchStatus::Archived));
        assert!(plan_hit_matches_status(&hit, PlanSearchStatus::All));
        assert_eq!(hit.age_ms, 100);
    }
}

#[test]
fn canonical_root_scopes_linked_worktree_units() {
    let mut plan = sample_plan();
    plan.project_path = "/linked-worktree".into();
    plan.canonical_project_path = "/main-project".into();
    assert_eq!(plan_project_root(&plan), Path::new("/main-project"));
    assert_eq!(
        IndexJob::Plan(plan.clone(), PlanIndexScope::Full).project_key(),
        project_scope(Path::new("/main-project"))
    );
    plan.canonical_project_path.clear();
    assert_eq!(plan_project_root(&plan), Path::new("/linked-worktree"));
}

#[test]
fn mirror_round_trip_indexes_plan_sections_and_tasks() {
    let root = tempfile::tempdir().unwrap();
    crate::service_plan::paths::mark_test_project_root(root.path());
    init_search_port(Arc::new(crate::service_plan::test_support::TestSearchPort));
    let mut plan = sample_plan();
    plan.project_path = root.path().display().to_string();
    plan.canonical_project_path = plan.project_path.clone();
    let queue = IndexQueue {
        state: Mutex::new(IndexQueueState::default()),
        ready: Condvar::new(),
    };
    queue.enqueue(IndexJob::Plan(plan.clone(), PlanIndexScope::Full));
    let (key, project, job) = queue.take();
    let IndexJob::Plan(snapshot, _) = job else {
        panic!("plan job")
    };
    mirror_plan(&snapshot, false).unwrap();
    queue.complete(key, project, false);
    let conn = open_index(root.path()).unwrap();
    let indexed = indexed_units_like(&conn, "planunit_%").unwrap();
    assert_eq!(indexed.len(), 6);
    assert!(indexed.contains_key("planunit_pl_decisions_task_beta"));
    let hits = search_project(
        root.path(),
        &PlanIndexQuery {
            query: "beta".into(),
            kind: None,
            scope: None,
            limit: Some(10),
        },
        PlanSearchStatus::Active,
    )
    .unwrap();
    assert!(hits.iter().any(|hit| hit.unit_id == "beta"));
}
