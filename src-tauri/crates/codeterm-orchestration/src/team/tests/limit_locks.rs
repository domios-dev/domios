use super::*;

#[test]
fn rename_team_core_refuses_locked_team_without_mutating_name() {
    let mut mux = MultiplexerState::empty();
    let workspace = mux.create_workspace("Project".into(), None).id;
    for created in 1..=7 {
        let mut team = Team::new(&workspace, created);
        team.name = Some("Original".into());
        mux.upsert_team(team);
    }
    let locked = mux.locks().teams.iter().next().unwrap().to_owned();
    let result = rename_team_core(
        &mut mux,
        &locked,
        "Changed".into(),
        &TeamMutationCaller::Human,
    );
    assert!(result.unwrap_err().starts_with("plan_limit_locked: "));
    assert_eq!(
        mux.get_team(&locked).unwrap().name.as_deref(),
        Some("Original")
    );
}
