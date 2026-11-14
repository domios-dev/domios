use super::*;
use crate::service_plan::store_db::PlanDatabase;
use rusqlite::Connection;

fn fixture() -> (tempfile::TempDir, PlanStore) {
    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    for id in ["pl_a", "pl_b"] {
        let mut plan = Plan::new(
            id.into(),
            directory.path().display().to_string(),
            OrchestratorRef::from_tab("owner"),
        );
        plan.id = id.into();
        store.save(&mut plan, 1).unwrap();
    }
    (directory, store)
}

fn change() -> AgentLifecyclePlanChange {
    AgentLifecyclePlanChange {
        tab_id: "owner".into(),
        preserve_owned_plans: false,
        retain_owned_plan_for_team_recovery: None,
        preserve_task_bindings_for_owner: None,
        planner_binding: None,
        plan_rebind: None,
    }
}

fn assert_matches_sql(store: &PlanStore) {
    let database = PlanDatabase::open(store.database_path_for_test()).unwrap();
    for id in ["pl_a", "pl_b"] {
        let stored = database.load_plan(id).unwrap().unwrap();
        assert_eq!(store.try_get(id).unwrap(), stored);
        assert_eq!(
            store.index.read()[id],
            database.load_index_row(id).unwrap().unwrap()
        );
        let listed = store
            .list_index()
            .unwrap()
            .into_iter()
            .find(|entry| entry.plan_id == id)
            .unwrap();
        assert_eq!(listed.status, stored.status);
        assert_eq!(listed.team_id, stored.team_id);
        assert_eq!(listed.updated_at, stored.updated_at);
    }
}

fn partial_restore(warm: bool) {
    let (_directory, store) = fixture();
    let transaction = store.apply_agent_lifecycle_change(change(), 2).unwrap();
    assert_eq!(transaction.changed_plan_ids(), ["pl_a", "pl_b"]);
    let retained = if warm {
        Some(store.get("pl_a").unwrap())
    } else {
        None
    };
    let connection = Connection::open(store.database_path_for_test()).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER refuse_restore BEFORE UPDATE ON plans
         WHEN NEW.id = 'pl_b' AND NEW.status = 'active'
         BEGIN SELECT RAISE(ABORT, 'restore B failed'); END;",
        )
        .unwrap();
    let error = store
        .rollback_agent_lifecycle_change(transaction)
        .unwrap_err();
    assert!(error.to_string().contains("restore B failed"));
    connection
        .execute_batch("DROP TRIGGER refuse_restore")
        .unwrap();
    assert_eq!(store.get("pl_a").unwrap().status, PlanStatus::Archived);
    assert_eq!(store.get("pl_b").unwrap().status, PlanStatus::Archived);
    if let Some(handle) = retained {
        assert_eq!(handle.status, PlanStatus::Archived);
    }
    assert_matches_sql(&store);
}

#[test]
fn partial_restore_refreshes_every_warm_plan_and_index() {
    partial_restore(true);
}

#[test]
fn partial_restore_refreshes_every_evicted_plan_and_index() {
    partial_restore(false);
}

#[test]
fn forward_restore_failure_refreshes_every_plan_and_index() {
    let (_directory, store) = fixture();
    let connection = Connection::open(store.database_path_for_test()).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER refuse_forward_or_restore BEFORE UPDATE ON plans
         WHEN (NEW.id = 'pl_b' AND NEW.status = 'archived')
           OR (NEW.id = 'pl_a' AND NEW.status = 'active')
         BEGIN SELECT RAISE(ABORT, 'injected batch failure'); END;",
        )
        .unwrap();
    let error = store.apply_agent_lifecycle_change(change(), 2).unwrap_err();
    assert!(
        error.to_string().contains("injected batch failure"),
        "{error}"
    );
    connection
        .execute_batch("DROP TRIGGER refuse_forward_or_restore")
        .unwrap();
    assert_eq!(store.get("pl_a").unwrap().status, PlanStatus::Active);
    assert_eq!(store.get("pl_b").unwrap().status, PlanStatus::Active);
    assert_matches_sql(&store);
}

#[test]
fn prepared_lifecycle_cas_refuses_concurrent_plan_edit_without_restoring_stale_data() {
    let (_directory, store) = fixture();
    let prepared = store
        .prepare_agent_lifecycle_changes(vec![change()], 2)
        .unwrap();
    let mut newer = store.get("pl_a").unwrap();
    newer.title = "concurrent edit".into();
    store.save(&mut newer, 3).unwrap();
    assert!(store
        .commit_prepared_agent_lifecycle_changes(prepared)
        .unwrap_err()
        .to_string()
        .contains("lifecycle conflict"));
    assert_eq!(store.get("pl_a").unwrap(), newer);
    assert_eq!(
        store.get("pl_b").unwrap().owner.tab_id.as_deref(),
        Some("owner")
    );
    assert_matches_sql(&store);
}

#[test]
fn prepared_lifecycle_cas_commits_once_and_rejects_a_second_stale_candidate() {
    let (_directory, store) = fixture();
    let first = store
        .prepare_agent_lifecycle_changes(vec![change()], 2)
        .unwrap();
    let stale = store
        .prepare_agent_lifecycle_changes(vec![change()], 2)
        .unwrap();
    assert_eq!(
        store
            .commit_prepared_agent_lifecycle_changes(first)
            .unwrap(),
        ["pl_a", "pl_b"]
    );
    assert!(store
        .commit_prepared_agent_lifecycle_changes(stale)
        .is_err());
    assert_eq!(store.get("pl_a").unwrap().status, PlanStatus::Archived);
    assert_matches_sql(&store);
}

#[test]
fn prepared_unarchive_cas_preserves_newer_plan_and_refuses_another_active_team_plan() {
    let (_directory, store) = fixture();
    let mut plan = store.get("pl_a").unwrap();
    plan.team_id = Some("team".into());
    plan.status = PlanStatus::Archived;
    store.save(&mut plan, 2).unwrap();
    let (prepared, team) = store.prepare_team_plan_unarchive("pl_a", 3).unwrap();
    assert_eq!(team, "team");
    plan.title = "concurrent title".into();
    store.save(&mut plan, 4).unwrap();
    assert!(store
        .commit_prepared_agent_lifecycle_changes(prepared)
        .unwrap_err()
        .to_string()
        .contains("conflict"));
    assert_eq!(store.get("pl_a").unwrap(), plan);
    let mut other = store.get("pl_b").unwrap();
    other.team_id = Some("team".into());
    store.save(&mut other, 5).unwrap();
    assert!(matches!(
        store.prepare_team_plan_unarchive("pl_a", 6),
        Err(PlanError::TeamAlreadyHasActivePlan { .. })
    ));
    assert_matches_sql(&store);
}
