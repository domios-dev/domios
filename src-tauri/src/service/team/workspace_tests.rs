use super::*;

use crate::service::test_fixture::ServiceFixture;

struct MovedTeam {
    mux: MultiplexerState,
    team_id: String,
    anchor: String,
    stay_behind: String,
    mint_workspace: String,
    moved_workspace: String,
}

/// Anchor minted in one workspace, then moved to another. Without the move the
/// stored and derived values coincide and nothing discriminates.
fn moved_team() -> MovedTeam {
    let (mut mux, anchor) = MultiplexerState::new();
    mux.set_orchestrator_role(&anchor).unwrap();
    let mint_workspace = mux.find_workspace_for_tab(&anchor).unwrap();
    mux.rename_workspace(&mint_workspace, "mint-project")
        .unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&anchor)
        .unwrap()
        .team_id;
    // move_tab refuses to empty a workspace, so the mint workspace keeps a tab.
    let stay_behind = mux.create_tab(&mint_workspace).unwrap();
    let moved_workspace = mux.create_workspace("moved-project".into(), None).id;
    mux.move_tab(&anchor, &mint_workspace, &moved_workspace)
        .unwrap();
    MovedTeam {
        mux,
        team_id,
        anchor,
        stay_behind,
        mint_workspace,
        moved_workspace,
    }
}

#[test]
fn team_workspace_derives_the_current_placement_not_the_mint_time_field() {
    let moved = moved_team();

    assert_eq!(
        moved.mux.get_team(&moved.team_id).unwrap().workspace_id,
        moved.mint_workspace,
        "the stored field stays mint-time provenance"
    );
    assert_ne!(moved.mint_workspace, moved.moved_workspace);
    assert_eq!(
        moved.mux.team_workspace(&moved.team_id).as_deref(),
        Some(moved.moved_workspace.as_str())
    );
}

#[test]
fn team_workspace_prefers_the_anchor_over_a_member_left_behind() {
    let mut moved = moved_team();
    let team_id = moved.team_id.clone();
    moved.mux.get_tab_mut(&moved.stay_behind).unwrap().team_id = Some(team_id);
    assert!(!moved
        .mux
        .get_tab(&moved.stay_behind)
        .unwrap()
        .anchors_team());

    assert_eq!(
        moved.mux.team_workspace(&moved.team_id).as_deref(),
        Some(moved.moved_workspace.as_str())
    );
}

#[test]
fn team_workspace_is_absent_for_an_unregistered_team() {
    let moved = moved_team();

    assert_eq!(moved.mux.team_workspace("team_missing"), None);
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn team_workspace_names_the_current_project_in_the_manager_bootstrap() {
    let moved = moved_team();
    let (team_id, anchor) = (moved.team_id.clone(), moved.anchor.clone());
    let mint_name = moved
        .mux
        .get_workspace(&moved.mint_workspace)
        .unwrap()
        .name
        .clone();
    let fixture = ServiceFixture::new(moved.mux);
    let mut params = crate::service::SpawnAgentParams {
        role: Some("Manager".into()),
        role_profile: Some(ORCHESTRATOR.into()),
        ..Default::default()
    };

    fixture
        .service
        .attach_manager_bootstrap(&team_id, Some(&anchor), &mut params)
        .unwrap();

    let bootstrap = params.manager_bootstrap.unwrap();
    assert_eq!(bootstrap.team_id, team_id);
    assert_eq!(mint_name, "mint-project");
    assert_eq!(bootstrap.project, "moved-project");
}
