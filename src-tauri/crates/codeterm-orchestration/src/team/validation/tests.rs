use super::*;
use std::cell::Cell;

fn binding(plan_id: &str, team_id: &str) -> PlanTeamBinding {
    PlanTeamBinding::new(plan_id.into(), Some(Some(team_id.into())))
}

fn validate(bindings: Vec<PlanTeamBinding>, live_teams: &[&str]) -> Result<(), TeamPlanError> {
    validate_bindings_with(
        || bindings,
        |_| live_teams.iter().map(|team| (*team).into()).collect(),
    )
}

#[test]
fn store_reads_finish_without_mux_guard_before_one_team_snapshot() {
    let mux = parking_lot::RwLock::new(BTreeSet::from(["team_a", "team_b"]));
    let store_reads = Cell::new(0);
    let snapshots = Cell::new(0);
    let read_store = || {
        assert!(mux.try_write().is_some(), "store read held a mux guard");
        assert_eq!(snapshots.get(), 0);
        store_reads.set(store_reads.get() + 1);
    };

    let result = validate_bindings_with(
        || {
            read_store();
            ["plan_a", "plan_b"]
                .into_iter()
                .map(|plan_id| {
                    read_store();
                    binding(
                        plan_id,
                        if plan_id == "plan_a" {
                            "team_a"
                        } else {
                            "team_b"
                        },
                    )
                })
                .collect()
        },
        |team_ids| {
            assert_eq!(store_reads.get(), 3);
            snapshots.set(snapshots.get() + 1);
            assert_eq!(team_ids, BTreeSet::from(["team_a".into(), "team_b".into()]));
            let guard = mux.read();
            assert!(mux.try_write().is_none());
            team_ids
                .into_iter()
                .filter(|team_id| guard.contains(team_id.as_str()))
                .collect()
        },
    );

    assert!(result.is_ok());
    assert_eq!(store_reads.get(), 3);
    assert_eq!(snapshots.get(), 1);
    assert!(mux.try_write().is_some());
}

#[test]
fn no_active_bindings_need_no_mux_snapshot() {
    assert!(validate_bindings_with(Vec::new, |_| panic!("unexpected mux read")).is_ok());
}

#[test]
fn missing_plan_remains_not_found() {
    assert!(matches!(
        validate(vec![PlanTeamBinding::new("plan_a".into(), None)], &[]),
        Err(TeamPlanError::Plan(PlanError::NotFound))
    ));
}

#[test]
fn absent_and_blank_team_bindings_still_require_a_team() {
    for team_id in [None, Some(String::new()), Some(" \t".into())] {
        assert!(matches!(
            validate(vec![PlanTeamBinding::new("plan_a".into(), Some(team_id))], &[]),
            Err(TeamPlanError::Plan(PlanError::ActivePlanRequiresTeam { plan_id }))
                if plan_id == "plan_a"
        ));
    }
}

#[test]
fn missing_team_keeps_the_adoption_error() {
    let Err(TeamPlanError::Team(error)) = validate(vec![binding("plan_a", "team_gone")], &[])
    else {
        panic!("expected missing Team error");
    };
    assert_eq!(
        error,
        "Plan plan_a references missing Team team_gone; adopt it into a live Team before resuming"
    );
}

#[test]
fn duplicate_binding_keeps_the_first_plan_and_conflict_details() {
    assert!(matches!(
        validate(vec![binding("plan_a", "team_a"), binding("plan_b", "team_a")], &["team_a"]),
        Err(TeamPlanError::Plan(PlanError::TeamAlreadyHasActivePlan {
            team_id,
            existing_plan_id,
            existing_title,
        })) if team_id == "team_a"
            && existing_plan_id == "plan_a"
            && existing_title == "conflicting active plan"
    ));
}

#[test]
fn earlier_missing_team_precedes_a_later_store_error() {
    assert!(matches!(
        validate(
            vec![
                binding("plan_a", "team_gone"),
                PlanTeamBinding::new("plan_b".into(), None)
            ],
            &[]
        ),
        Err(TeamPlanError::Team(_))
    ));
}

#[test]
fn earlier_conflict_precedes_a_later_store_error() {
    assert!(matches!(
        validate(
            vec![
                binding("plan_a", "team_a"),
                binding("plan_b", "team_a"),
                PlanTeamBinding::new("plan_c".into(), None)
            ],
            &["team_a"]
        ),
        Err(TeamPlanError::Plan(
            PlanError::TeamAlreadyHasActivePlan { .. }
        ))
    ));
}

#[test]
fn team_identity_is_not_trimmed() {
    assert!(validate(vec![binding("plan_a", " team_a ")], &[" team_a "]).is_ok());
    assert!(matches!(
        validate(vec![binding("plan_a", " team_a ")], &["team_a"]),
        Err(TeamPlanError::Team(_))
    ));
}

#[test]
fn service_validation_uses_live_team_state_and_skips_archived_and_orphaned_plans() {
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::plan::{OrchestratorRef, PlanOrphan};

    let (mut mux, owner) = MultiplexerState::new();
    let team_id = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
    let fixture = crate::test_fixture::ServiceFixture::new(mux);
    let svc = &fixture.service;
    let plan = svc
        .plan_store
        .create_plan_for_team(
            "Validation".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab(&owner),
            team_id,
            vec![],
            1,
        )
        .unwrap();

    assert!(validate_active_team_plan_bindings(svc).is_ok());
    assert!(validate_plan_team_binding(svc, &plan.id).is_ok());
    assert!(matches!(
        validate_plan_team_binding(svc, "absent"),
        Err(TeamPlanError::Plan(PlanError::NotFound))
    ));

    svc.plan_store
        .with_plan(&plan.id, 2, |plan| {
            plan.team_id = Some("team_gone".into());
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        validate_active_team_plan_bindings(svc),
        Err(TeamPlanError::Team(_))
    ));
    assert!(matches!(
        validate_plan_team_binding(svc, &plan.id),
        Err(TeamPlanError::Team(_))
    ));

    svc.plan_store
        .with_plan(&plan.id, 3, |plan| {
            plan.status = PlanStatus::Archived;
            Ok(())
        })
        .unwrap();
    assert!(validate_active_team_plan_bindings(svc).is_ok());
    assert!(validate_plan_team_binding(svc, &plan.id).is_ok());

    svc.plan_store
        .with_plan(&plan.id, 4, |plan| {
            plan.status = PlanStatus::Active;
            plan.team_id = None;
            plan.orphaned = Some(PlanOrphan {
                team_id: "team_gone".into(),
                reason: "closed".into(),
                unfinished_task_ids: vec![],
                at: 4,
            });
            Ok(())
        })
        .unwrap();
    assert!(validate_active_team_plan_bindings(svc).is_ok());
    assert!(validate_plan_team_binding(svc, &plan.id).is_ok());
}
