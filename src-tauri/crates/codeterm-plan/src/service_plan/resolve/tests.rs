use super::{RefKind, ResolveError};
use crate::plan::OrchestratorRef;
use crate::service_plan::{NewTask, PlanStore};

fn store_with(dir: &std::path::Path) -> PlanStore {
    PlanStore::new_for_test(dir)
}

fn mk_plan(store: &PlanStore, dir: &std::path::Path, title: &str) -> String {
    store
        .create_plan(
            title.into(),
            dir.display().to_string(),
            OrchestratorRef::from_tab("p"),
            vec![],
            1000,
        )
        .unwrap()
        .id
}

fn add_task(store: &PlanStore, plan_id: &str, name: &str, handle: &str) -> String {
    store
        .add_task(
            plan_id,
            NewTask {
                name: name.into(),
                kind: "task".into(),
                handle: Some(handle.into()),
                competition: None,
                ..Default::default()
            },
            2000,
        )
        .unwrap()
}

#[test]
fn minted_ids_are_short_and_prefixed() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with(dir.path());
    let plan_id = mk_plan(&store, dir.path(), "P");
    assert!(
        plan_id.starts_with("pl_"),
        "plan id keeps prefix: {plan_id}"
    );
    let slug = plan_id.split_once('_').unwrap().1;
    assert!(
        (6..=8).contains(&slug.len()),
        "plan slug must be 6-8 chars, got {} ({plan_id})",
        slug.len()
    );
    let task_id = add_task(&store, &plan_id, "T", "alpha");
    let tslug = task_id.split_once('_').unwrap().1;
    assert!(task_id.starts_with("task_"));
    assert!((6..=8).contains(&tslug.len()), "task slug 6-8: {task_id}");
}

#[test]
fn resolves_plan_by_full_id_and_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with(dir.path());
    let plan_id = mk_plan(&store, dir.path(), "P");
    assert_eq!(store.resolve_plan_id(&plan_id).unwrap(), plan_id);
    let prefix = &plan_id[..plan_id.len() - 2];
    assert_eq!(store.resolve_plan_id(prefix).unwrap(), plan_id);
}

#[test]
fn resolves_task_parent_free_by_handle_and_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with(dir.path());
    let plan_a = mk_plan(&store, dir.path(), "A");
    let plan_b = mk_plan(&store, dir.path(), "B");
    let task_a = add_task(&store, &plan_a, "Alpha", "alpha");
    let _task_b = add_task(&store, &plan_b, "Beta", "beta");

    // Handle, no plan ref -> resolves across plans.
    let r = store.resolve_task_ref(None, "alpha").unwrap();
    assert_eq!(r.plan_id, plan_a);
    assert_eq!(r.task_id, task_a);

    // 4-char id prefix, no plan ref.
    let prefix = &task_a[..9]; // "task_" + 4 chars
    let r = store.resolve_task_ref(None, prefix).unwrap();
    assert_eq!(r.task_id, task_a);

    // Full id still resolves.
    let r = store.resolve_task_ref(None, &task_a).unwrap();
    assert_eq!(r.task_id, task_a);
}

#[test]
fn ambiguous_handle_prefix_lists_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with(dir.path());
    let plan = mk_plan(&store, dir.path(), "P");
    add_task(&store, &plan, "Login", "auth-login");
    add_task(&store, &plan, "Logout", "auth-logout");

    let err = store.resolve_task_ref(Some(&plan), "auth").unwrap_err();
    match err {
        ResolveError::Ambiguous {
            kind, candidates, ..
        } => {
            assert_eq!(kind, RefKind::Task);
            assert_eq!(candidates.len(), 2);
        }
        other => panic!("expected ambiguity, got {other:?}"),
    }
}

#[test]
fn exact_handle_wins_over_prefix_of_another() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with(dir.path());
    let plan = mk_plan(&store, dir.path(), "P");
    let short = add_task(&store, &plan, "Auth", "auth");
    add_task(&store, &plan, "Auth login", "auth-login");
    // "auth" is an exact handle AND a prefix of "auth-login"; exact wins, no ambiguity.
    let r = store.resolve_task_ref(Some(&plan), "auth").unwrap();
    assert_eq!(r.task_id, short);
}

#[test]
fn unknown_ref_never_guesses() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with(dir.path());
    let plan = mk_plan(&store, dir.path(), "P");
    add_task(&store, &plan, "Alpha", "alpha");
    match store.resolve_task_ref(None, "zzz-nope").unwrap_err() {
        ResolveError::NotFound { kind, .. } => assert_eq!(kind, RefKind::Task),
        other => panic!("expected not found, got {other:?}"),
    }
}

#[test]
fn resolves_section_parent_free() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with(dir.path());
    let plan = mk_plan(&store, dir.path(), "P");
    let sid = store
        .add_section(&plan, "Design", "", None, &[], None, Some("design"), 2000)
        .unwrap();
    let r = store.resolve_section_ref(None, "design").unwrap();
    assert_eq!(r.plan_id, plan);
    assert_eq!(r.section_id, sid);
    let r = store.resolve_section_ref(None, &sid).unwrap();
    assert_eq!(r.section_id, sid);
}

#[test]
fn id_taken_reports_all_entity_kinds() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with(dir.path());
    let plan = mk_plan(&store, dir.path(), "P");
    let task = add_task(&store, &plan, "T", "alpha");
    assert!(store.id_taken(&plan).unwrap());
    assert!(store.id_taken(&task).unwrap());
    assert!(!store.id_taken("task_notreal").unwrap());
}
