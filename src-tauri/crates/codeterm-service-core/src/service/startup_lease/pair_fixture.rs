use crate::multiplexer::state::MultiplexerState;

pub struct PairFixture {
    pub mux: MultiplexerState,
    pub workspace_id: String,
    pub team_id: String,
    pub creator: String,
    pub orchestrator: String,
    pub planner: String,
}

#[allow(clippy::new_without_default)]
impl PairFixture {
    pub fn new() -> Self {
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
}
