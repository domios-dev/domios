use std::sync::{Arc, Barrier};

use crate::plan::{OrchestratorRef, PlanMode, PlanStatus};
use crate::service_plan::{retrospective, PlanError, PlanStore};
use tempfile::TempDir;

fn store() -> (TempDir, PlanStore) {
    let dir = TempDir::new().unwrap();
    let s = PlanStore::new_for_test(dir.path());
    (dir, s)
}

fn create(
    s: &PlanStore,
    dir: &TempDir,
    title: &str,
    team: &str,
    now: u64,
) -> Result<crate::plan::Plan, PlanError> {
    s.create_plan_for_team(
        title.into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
        team.to_string(),
        vec![],
        now,
    )
}

#[test]
fn teamless_creator_is_rejected_without_a_fallback_plan() {
    let (dir, s) = store();
    let error = create(&s, &dir, "A", "", 1).unwrap_err();
    assert!(matches!(error, PlanError::ActivePlanRequiresTeam { .. }));
    assert!(s.list_index().is_empty());
}

#[test]
fn team_bound_creation_persists_team_id() {
    let (dir, s) = store();
    let plan = create(&s, &dir, "Bound", "team_alpha", 1).unwrap();
    assert_eq!(plan.team_id.as_deref(), Some("team_alpha"));
    assert_eq!(
        s.active_plan_for_team("team_alpha"),
        Some((plan.id.clone(), "Bound".into()))
    );
    // Exposed in the plan index.
    let entry = s
        .list_index()
        .into_iter()
        .find(|e| e.plan_id == plan.id)
        .unwrap();
    assert_eq!(entry.team_id.as_deref(), Some("team_alpha"));
}

#[test]
fn team_close_archives_plan_until_atomic_adoption() {
    let (dir, s) = store();
    let plan = create(&s, &dir, "Bound", "team_closed", 1).unwrap();

    assert_eq!(
        s.archive_and_detach_team_plans("team_closed", 2).unwrap(),
        vec![plan.id.clone()]
    );
    let archived = s.get(&plan.id).unwrap();
    assert_eq!(archived.team_id, None);
    assert_eq!(archived.status, PlanStatus::Archived);

    assert!(matches!(
        s.resume_plan(&plan.id, 3),
        Err(PlanError::ActivePlanRequiresTeam { .. })
    ));
    let rebound = s
        .adopt_plan_for_team(
            &plan.id,
            "team_new",
            OrchestratorRef::from_tab("new-owner"),
            4,
        )
        .unwrap();
    assert_eq!(rebound.team_id.as_deref(), Some("team_new"));
    assert_eq!(rebound.status, PlanStatus::Active);
    assert_eq!(rebound.owner.tab_id.as_deref(), Some("new-owner"));
}

#[test]
fn dangling_team_reconciliation_archives_every_active_orphan() {
    let (dir, s) = store();
    let parked = create(&s, &dir, "Parked", "team_missing", 1).unwrap();
    s.set_mode(&parked.id, PlanMode::Paused, 2).unwrap();
    let done = create(&s, &dir, "Done", "team_done", 3).unwrap();
    s.set_mode(&done.id, PlanMode::Complete, 4).unwrap();
    let archived = create(&s, &dir, "Archived", "team_archived", 5).unwrap();
    s.update_plan(&archived.id, None, Some(PlanStatus::Archived), None, 6)
        .unwrap();
    let healthy = create(&s, &dir, "Healthy", "team_live", 7).unwrap();
    let live = std::collections::BTreeSet::from(["team_live".to_string()]);

    let mut expected = vec![done.id.clone(), parked.id.clone()];
    expected.sort();
    assert_eq!(
        s.archive_active_plans_without_live_team(&live, 8).unwrap(),
        expected
    );
    assert_eq!(s.get(&parked.id).unwrap().status, PlanStatus::Archived);
    assert_eq!(s.get(&done.id).unwrap().status, PlanStatus::Archived);
    assert_eq!(s.get(&archived.id).unwrap().status, PlanStatus::Archived);
    assert_eq!(
        s.get(&healthy.id).unwrap().team_id.as_deref(),
        Some("team_live")
    );
    assert!(s
        .archive_active_plans_without_live_team(&live, 9)
        .unwrap()
        .is_empty());
}

#[test]
fn second_active_plan_hard_refused() {
    let (dir, s) = store();
    let first = create(&s, &dir, "First", "team_alpha", 1).unwrap();
    let before = s.list_index().len();
    let err = create(&s, &dir, "Second", "team_alpha", 2).unwrap_err();
    match &err {
        PlanError::TeamAlreadyHasActivePlan {
            team_id,
            existing_plan_id,
            existing_title,
        } => {
            assert_eq!(team_id, "team_alpha");
            assert_eq!(existing_plan_id, &first.id);
            assert_eq!(existing_title, "First");
        }
        other => panic!("expected hard refuse, got {other:?}"),
    }
    let msg = err.to_string();
    assert!(msg.contains(&first.id), "error must name the existing plan");
    assert!(msg.contains("team_alpha"), "error must name the team");
    assert!(
        msg.to_lowercase().contains("another team"),
        "error must instruct to use another Team: {msg}"
    );
    // Hard refuse leaves the store unchanged — no teamless fallback plan.
    assert_eq!(s.list_index().len(), before);
}

#[test]
fn concurrent_team_plan_creation_allows_exactly_one() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(PlanStore::new_for_test(dir.path()));
    let barrier = Arc::new(Barrier::new(3));
    let root = dir.path().display().to_string();
    let spawn = |title: &'static str, now| {
        let store = store.clone();
        let barrier = barrier.clone();
        let root = root.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.create_plan_for_team(
                title.into(),
                root,
                OrchestratorRef::from_tab(title),
                "team_alpha".into(),
                vec![],
                now,
            )
        })
    };
    let first = spawn("First", 1);
    let second = spawn("Second", 2);
    barrier.wait();
    let results = [first.join().unwrap(), second.join().unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(PlanError::TeamAlreadyHasActivePlan { .. })))
            .count(),
        1
    );
    assert_eq!(store.list_index().len(), 1);
}

#[test]
fn concurrent_team_plan_create_and_bind_allow_exactly_one() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(PlanStore::new_for_test(dir.path()));
    let mut teamless = crate::plan::Plan::new(
        "Existing".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut teamless, 1).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let bind_store = store.clone();
    let bind_barrier = barrier.clone();
    let bind_plan_id = teamless.id.clone();
    let bind = std::thread::spawn(move || {
        bind_barrier.wait();
        bind_store.bind_plan_to_team(&bind_plan_id, "team_alpha", 2)
    });
    let create_store = store.clone();
    let create_barrier = barrier.clone();
    let root = dir.path().display().to_string();
    let create = std::thread::spawn(move || {
        create_barrier.wait();
        create_store.create_plan_for_team(
            "New".into(),
            root,
            OrchestratorRef::from_tab("orch"),
            "team_alpha".into(),
            vec![],
            3,
        )
    });
    barrier.wait();
    let results = [bind.join().unwrap(), create.join().unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(PlanError::TeamAlreadyHasActivePlan { .. })))
            .count(),
        1
    );
    assert!(store.active_plan_for_team("team_alpha").is_some());
}

#[test]
fn different_team_gets_its_own_plan() {
    let (dir, s) = store();
    create(&s, &dir, "Alpha", "team_alpha", 1).unwrap();
    let beta = create(&s, &dir, "Beta", "team_beta", 2).unwrap();
    assert_eq!(beta.team_id.as_deref(), Some("team_beta"));
}

/// Advisory, not a gate: a missing retrospective must never trap closure.
#[test]
fn archive_succeeds_without_retrospective() {
    let (dir, s) = store();
    let plan = create(&s, &dir, "Retro advisory", "team_alpha", 1).unwrap();
    assert!(!retrospective::has_retrospective(&s.get(&plan.id).unwrap()));

    s.archive(&plan.id, 2).unwrap();

    assert_eq!(s.get(&plan.id).unwrap().status, PlanStatus::Archived);
}

#[test]
fn archiving_frees_the_team_for_a_new_plan() {
    let (dir, s) = store();
    let first = create(&s, &dir, "First", "team_alpha", 1).unwrap();
    assert!(matches!(
        create(&s, &dir, "Second", "team_alpha", 2),
        Err(PlanError::TeamAlreadyHasActivePlan { .. })
    ));
    s.archive(&first.id, 3).unwrap();
    // The 1:1 rule counts only ACTIVE plans — an archived one no longer blocks.
    let second = create(&s, &dir, "Second", "team_alpha", 4).unwrap();
    assert_eq!(second.team_id.as_deref(), Some("team_alpha"));
    assert_ne!(first.id, second.id);
}
