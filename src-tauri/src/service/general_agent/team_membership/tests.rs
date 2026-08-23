use super::*;

use codeterm_types::team::general_agent_membership::GENERAL_AGENT_TEAM_MEMBERSHIP_VIOLATION;

use crate::multiplexer::state::MultiplexerState;

fn mux_with_general_agent() -> (MultiplexerState, String, String) {
    let (mut mux, ordinary) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&ordinary).unwrap();
    let general = mux.create_tab(&workspace_id).unwrap();
    let tab = mux.get_tab_mut(&general).unwrap();
    super::super::identity::apply_general_agent_kind(tab);
    (mux, general, ordinary)
}

#[test]
fn general_agent_team_guard_refuses_by_role_kind_on_both_attempts() {
    let (mux, general, _) = mux_with_general_agent();
    for attempt in [MembershipAttempt::Join, MembershipAttempt::ExerciseMembership] {
        let refusal = refuse_general_agent_team_membership(
            &mux,
            &general,
            attempt,
            &CorrectiveShape::default(),
        )
        .expect_err("the General Agent kind is refused Team membership");
        assert_eq!(
            refusal.violation_id,
            GENERAL_AGENT_TEAM_MEMBERSHIP_VIOLATION
        );
        assert_eq!(refusal.tab_id, general);
        assert_eq!(refusal.role_kind_id, GENERAL_AGENT_KIND);
    }
}

#[test]
fn general_agent_team_guard_leaves_every_other_tab_alone() {
    let (mux, _, ordinary) = mux_with_general_agent();
    assert!(refuse_general_agent_team_membership(
        &mux,
        &ordinary,
        MembershipAttempt::Join,
        &CorrectiveShape::default(),
    )
    .is_ok());
}

#[test]
fn general_agent_team_guard_defers_unknown_and_blank_callers_to_their_own_lookup() {
    let (mux, _, _) = mux_with_general_agent();
    for tab_id in ["", "   ", "tab_does_not_exist"] {
        assert!(refuse_general_agent_team_membership(
            &mux,
            tab_id,
            MembershipAttempt::Join,
            &CorrectiveShape::default(),
        )
        .is_ok());
    }
}

#[test]
fn general_agent_team_guard_shape_points_at_the_tab_own_workspace() {
    let (mux, general, _) = mux_with_general_agent();
    let workspace_id = mux.find_workspace_for_tab(&general).unwrap();
    let command = shape_for_tab(&mux, &general).pair_command();
    assert!(command.contains(&workspace_id), "{command}");
}

#[test]
fn general_agent_team_guard_stops_tracking_a_tab_that_loses_the_kind() {
    let (mut mux, general, _) = mux_with_general_agent();
    let tab = mux.get_tab_mut(&general).unwrap();
    tab.role = None;
    tab.role_profile = None;
    assert!(
        refuse_general_agent_team_membership(
            &mux,
            &general,
            MembershipAttempt::Join,
            &CorrectiveShape::default(),
        )
        .is_ok(),
        "identity is read live from the role kind, never cached per tab id"
    );
}

#[test]
fn general_agent_team_guard_refuses_parenting_a_role_under_the_general_agent() {
    let (mux, general, ordinary) = mux_with_general_agent();

    let refusal = refuse_parenting_under_general_agent(&mux, Some(&general))
        .expect_err("placement mints the parent's Team onto the parent");
    assert_eq!(
        refusal.violation_id,
        GENERAL_AGENT_TEAM_MEMBERSHIP_VIOLATION
    );

    assert!(refuse_parenting_under_general_agent(&mux, Some(&ordinary)).is_ok());
    assert!(
        refuse_parenting_under_general_agent(&mux, None).is_ok(),
        "an anchoring placement has no parent and is never this guard's business"
    );
}
