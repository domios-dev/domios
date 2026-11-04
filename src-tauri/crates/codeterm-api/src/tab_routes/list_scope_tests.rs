#[cfg(test)]
mod explicit_address_scope_tests {
    #[test]
    fn explicit_address_scope_mesh_all_workspaces_matches_the_peer_workspace_list() {
        let (mut state, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
        state.set_limit_gate(codeterm_entitlements::LimitGate::dormant());
        let second = state.create_workspace("second".into(), None);
        state.create_tab(&second.id).unwrap();
        let third = state.create_workspace("third".into(), None);
        state.create_tab(&third.id).unwrap();
        let snapshot = state.snapshot();
        let expected = snapshot
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.tabs.iter().cloned())
            .collect::<std::collections::HashSet<_>>();

        let actual = codeterm_service_core::dispatch::build_tab_summary_json(
            &snapshot, None, None, false, false,
        )
        .into_iter()
        .filter_map(|tab| tab["id"].as_str().map(str::to_owned))
        .collect::<std::collections::HashSet<_>>();

        assert_eq!(actual, expected);
    }
}

#[cfg(test)]
mod default_scope_contract_tests {
    use super::super::TabListQuery;
    use codeterm_service_core::dispatch::build_tab_summary_json;
    use codeterm_service_core::multiplexer::state::MultiplexerState;

    #[test]
    fn tabs_default_to_all_workspaces() {
        let query = TabListQuery::default();
        let (mut state, _) = MultiplexerState::new();
        let second = state.create_workspace("second".into(), None);
        state.create_tab(&second.id).unwrap();

        let tabs = build_tab_summary_json(
            &state.snapshot(),
            query.workspace_id.as_deref(),
            None,
            !query.all_workspaces && query.focused_workspace,
            query.agents,
        );

        assert_eq!(tabs.len(), 2);
        assert!(tabs.iter().all(|tab| tab["scope"] == "all_workspaces"));
    }

    #[test]
    fn tabs_explicit_workspace_scope() {
        let (mut state, _) = MultiplexerState::new();
        let first = state.snapshot().workspaces[0].id.clone();
        let second = state.create_workspace("second".into(), None);
        state.create_tab(&second.id).unwrap();
        let query = TabListQuery {
            workspace_id: Some(first.clone()),
            ..TabListQuery::default()
        };

        let tabs = build_tab_summary_json(
            &state.snapshot(),
            query.workspace_id.as_deref(),
            None,
            !query.all_workspaces && query.focused_workspace,
            query.agents,
        );

        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0]["scope"], "workspace");
        assert_eq!(tabs[0]["scope_workspace_id"], first);
        assert_eq!(tabs[0]["other_workspace_tab_count"], 1);
    }
}
