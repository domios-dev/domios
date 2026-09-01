use super::*;

use codeterm_types::team::Team;

struct PairFixture {
    mux: MultiplexerState,
    workspace_id: String,
    team_id: String,
    creator: String,
    orchestrator: String,
    planner: String,
}

impl PairFixture {
    fn new() -> Self {
        let (mut mux, creator) = MultiplexerState::new();
        let workspace_id = mux.find_workspace_for_tab(&creator).unwrap();
        let orchestrator = mux.create_tab(&workspace_id).unwrap();
        let planner = mux.create_tab(&workspace_id).unwrap();
        let team_id = mux
            .resolve_or_mint_team_for_parent(&orchestrator)
            .unwrap()
            .team_id;
        mux.get_tab_mut(&planner).unwrap().team_id = Some(team_id.clone());
        Self {
            mux,
            workspace_id,
            team_id,
            creator,
            orchestrator,
            planner,
        }
    }

    fn lease(&self) -> StartupTeamLease {
        StartupTeamLease {
            team_id: self.team_id.clone(),
            creator_tab_id: self.creator.clone(),
            member_tab_ids: vec![self.orchestrator.clone(), self.planner.clone()],
        }
    }

    fn close_members(&mut self) {
        let members = vec![self.orchestrator.clone(), self.planner.clone()];
        for tab_id in members {
            self.mux.close_tab(&tab_id).unwrap();
        }
    }

    fn reap(&mut self) -> TeamReapOutcome {
        let lease = self.lease();
        reap_owned_forming_team(&mut self.mux, &lease.team_id, &lease.member_tab_ids)
    }
}

fn child_of(mux: &MultiplexerState, tab_id: &str) -> Option<ChildStartupState> {
    mux.get_tab(tab_id).map(|tab| ChildStartupState {
        attempt_id: tab.pty_id.clone(),
        team_id: tab.team_id.clone(),
    })
}

#[test]
fn creator_hold_requires_matching_pair_attempt() {
    let mut fixture = PairFixture::new();
    let general_agent = fixture.mux.create_tab(&fixture.workspace_id).unwrap();
    fixture.mux.get_tab_mut(&fixture.planner).unwrap().pty_id = Some("launch-a".into());
    let supervised = StartupAuthority::new(&fixture.creator, &fixture.planner, "launch-a")
        .in_team(Some(fixture.team_id.clone()));
    let child = child_of(&fixture.mux, &fixture.planner);

    let granted = authorize_startup_hold(
        &fixture.creator,
        &fixture.planner,
        "launch-a",
        Some(&supervised),
        child.as_ref(),
    )
    .expect("the recorded creator holds its own live attempt");
    assert_eq!(granted, supervised);
    assert_eq!(granted.provenance()["team_id"], fixture.team_id);
    assert_eq!(granted.provenance()["creator_tab_id"], fixture.creator);

    for (caller, expected) in [
        (
            fixture.orchestrator.as_str(),
            StartupHoldRefusal::CreatorMismatch {
                creator_tab_id: fixture.creator.clone(),
            },
        ),
        (
            general_agent.as_str(),
            StartupHoldRefusal::CreatorMismatch {
                creator_tab_id: fixture.creator.clone(),
            },
        ),
    ] {
        let refusal = authorize_startup_hold(
            caller,
            &fixture.planner,
            "launch-a",
            Some(&supervised),
            child.as_ref(),
        )
        .expect_err("only the recorded creator may hold");
        assert_eq!(refusal, expected);
        assert_eq!(refusal.reason(), "creator_mismatch");
        let provenance =
            refusal.provenance(caller, &fixture.planner, "launch-a", Some(&supervised));
        assert_eq!(provenance["held"], false);
        assert_eq!(provenance["caller_tab_id"], caller);
        assert_eq!(provenance["authority"]["creator_tab_id"], fixture.creator);
    }

    let stale = authorize_startup_hold(
        &fixture.creator,
        &fixture.planner,
        "launch-old",
        Some(&supervised),
        child.as_ref(),
    )
    .expect_err("a stale attempt id is refused");
    assert_eq!(stale.reason(), "attempt_replaced");

    fixture.mux.get_tab_mut(&fixture.planner).unwrap().pty_id = Some("launch-b".into());
    let replaced = authorize_startup_hold(
        &fixture.creator,
        &fixture.planner,
        "launch-a",
        Some(&supervised),
        child_of(&fixture.mux, &fixture.planner).as_ref(),
    )
    .expect_err("the same tab with a replaced attempt is refused");
    assert_eq!(
        replaced,
        StartupHoldRefusal::AttemptReplaced {
            current_attempt_id: Some("launch-b".into())
        }
    );

    assert_eq!(
        authorize_startup_hold(
            &fixture.creator,
            &fixture.planner,
            "launch-a",
            None,
            child.as_ref()
        )
        .expect_err("no supervised record means no authority")
        .reason(),
        "no_supervised_attempt"
    );
    assert_eq!(
        authorize_startup_hold(
            &fixture.creator,
            "gone",
            "launch-a",
            Some(&supervised),
            None
        )
        .expect_err("a missing child tab is refused")
        .reason(),
        "unknown_child_tab"
    );
}

#[test]
fn hold_refuses_a_child_that_left_the_leased_team() {
    let mut fixture = PairFixture::new();
    fixture.mux.get_tab_mut(&fixture.planner).unwrap().pty_id = Some("launch-a".into());
    fixture.mux.get_tab_mut(&fixture.planner).unwrap().team_id = Some("team_other".into());
    let supervised = StartupAuthority::new(&fixture.creator, &fixture.planner, "launch-a")
        .in_team(Some(fixture.team_id.clone()));

    let refusal = authorize_startup_hold(
        &fixture.creator,
        &fixture.planner,
        "launch-a",
        Some(&supervised),
        child_of(&fixture.mux, &fixture.planner).as_ref(),
    )
    .expect_err("a child outside the leased Team is refused");
    assert_eq!(
        refusal,
        StartupHoldRefusal::TeamMismatch {
            leased_team_id: fixture.team_id.clone(),
            child_team_id: Some("team_other".into()),
        }
    );
}

#[test]
fn policy_startup_reaps_only_owned_empty_forming_team() {
    let mut fixture = PairFixture::new();
    assert_eq!(fixture.reap(), TeamReapOutcome::MemberStillLive);

    fixture.mux.close_tab(&fixture.planner).unwrap();
    assert_eq!(
        fixture.reap(),
        TeamReapOutcome::MemberStillLive,
        "one owned member still live keeps the Team"
    );

    fixture.mux.close_tab(&fixture.orchestrator).unwrap();
    assert_eq!(fixture.reap(), TeamReapOutcome::Reaped);
    assert!(fixture.mux.get_team(&fixture.team_id).is_none());
    assert_eq!(
        fixture.reap(),
        TeamReapOutcome::TeamGone,
        "reaping is idempotent"
    );
}

#[test]
fn reaping_refuses_active_adopted_and_unowned_teams() {
    let mut fixture = PairFixture::new();
    let adopted = fixture.mux.create_tab(&fixture.workspace_id).unwrap();
    fixture.mux.get_tab_mut(&adopted).unwrap().team_id = Some(fixture.team_id.clone());
    fixture.close_members();
    assert_eq!(
        fixture.reap(),
        TeamReapOutcome::UnownedMember,
        "a Team holding a tab the reservation never claimed is never reaped"
    );

    fixture.mux.close_tab(&adopted).unwrap();
    let mut active = fixture.mux.get_team(&fixture.team_id).unwrap().clone();
    active.lifecycle = TeamLifecycle::Active;
    fixture.mux.upsert_team(active);
    assert_eq!(fixture.reap(), TeamReapOutcome::NotForming);

    let mut forming = fixture.mux.get_team(&fixture.team_id).unwrap().clone();
    forming.lifecycle = TeamLifecycle::Forming;
    fixture.mux.upsert_team(forming);
    let mismatched = StartupTeamLease {
        team_id: "team_elsewhere".into(),
        creator_tab_id: fixture.creator.clone(),
        member_tab_ids: vec![fixture.orchestrator.clone()],
    };
    assert_eq!(
        reap_owned_forming_team(
            &mut fixture.mux,
            &mismatched.team_id,
            &mismatched.member_tab_ids
        ),
        TeamReapOutcome::TeamGone
    );
    assert!(fixture.mux.get_team(&fixture.team_id).is_some());
}

#[test]
fn workspace_close_preserves_unowned_teams() {
    let mut fixture = PairFixture::new();
    let manual_anchor = fixture.mux.create_tab(&fixture.workspace_id).unwrap();
    let manual_team = Team::new(fixture.workspace_id.clone(), 1);
    let manual_team_id = manual_team.id.clone();
    fixture.mux.upsert_team(manual_team);
    fixture.mux.get_tab_mut(&manual_anchor).unwrap().team_id = Some(manual_team_id.clone());

    let empty_manual_team = Team::new(fixture.workspace_id.clone(), 2);
    let empty_manual_team_id = empty_manual_team.id.clone();
    fixture.mux.upsert_team(empty_manual_team);

    let leases = vec![fixture.lease()];
    assert!(
        reap_orphaned_leases(&mut fixture.mux, &leases).is_empty(),
        "a sweep before the members are gone reaps nothing"
    );

    fixture.close_members();
    let reaped = reap_orphaned_leases(&mut fixture.mux, &leases);

    assert_eq!(reaped, vec![fixture.team_id.clone()]);
    assert!(fixture.mux.get_team(&fixture.team_id).is_none());
    assert!(
        fixture.mux.get_team(&manual_team_id).is_some(),
        "a manually created Team with a live member survives"
    );
    assert!(
        fixture.mux.get_team(&empty_manual_team_id).is_some(),
        "an empty Team with no lease is never reaped"
    );
}

#[test]
fn release_owned_reservation_detaches_only_its_own_members() {
    let mut fixture = PairFixture::new();
    let foreign = fixture.mux.create_tab(&fixture.workspace_id).unwrap();
    fixture.mux.get_tab_mut(&foreign).unwrap().team_id = Some("team_other".into());
    let lease = fixture.lease();

    let outcome =
        release_owned_reservation(&mut fixture.mux, &lease.team_id, &lease.member_tab_ids);

    assert_eq!(outcome, TeamReapOutcome::Reaped);
    assert!(fixture.mux.get_team(&fixture.team_id).is_none());
    assert!(fixture
        .mux
        .get_tab(&fixture.orchestrator)
        .unwrap()
        .team_id
        .is_none());
    assert_eq!(
        fixture.mux.get_tab(&foreign).unwrap().team_id.as_deref(),
        Some("team_other"),
        "a tab outside the reservation keeps its Team"
    );
}

#[test]
fn normal_team_close_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let plan_store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let events = crate::api::events::EventBroadcaster::new();
    let mut fixture = PairFixture::new();
    fixture
        .mux
        .set_orchestrator_role(&fixture.orchestrator)
        .unwrap();
    let team_id = fixture.team_id.clone();

    let (closed, _cleanup, _) = crate::service::close_team_with_state_change(
        &mut fixture.mux,
        &events,
        &plan_store,
        &team_id,
        &crate::service::TeamMutationCaller::AgentTab(fixture.orchestrator.clone()),
        1,
    )
    .expect("an ordinary team close still cascades a forming, leased Team");

    assert_eq!(closed.id, team_id);
    assert!(fixture.mux.get_team(&team_id).is_none());
    assert!(fixture.mux.get_tab(&fixture.orchestrator).is_none());
    assert!(fixture.mux.get_tab(&fixture.planner).is_none());
}
