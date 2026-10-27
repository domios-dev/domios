use super::*;
use crate::multiplexer::{state::MultiplexerState, types::AgentRole};
use crate::service::test_fixture::ServiceFixture;

#[test]
fn authority_decision_table_refuses_self_foreign_leaf_and_stale_callers() {
    for self_target in [false, true] {
        for current_launch in [false, true] {
            for anchors_team in [false, true] {
                for caller_team in [None, Some("own")] {
                    for target_team in [None, Some("own"), Some("foreign")] {
                        for live_team in [false, true] {
                            for destination_allowed in [false, true] {
                                let facts = AuthorityFacts {
                                    caller_id: "caller",
                                    target_id: if self_target { "caller" } else { "target" },
                                    current_launch,
                                    anchors_team,
                                    caller_team,
                                    target_team,
                                    live_team,
                                    destination_allowed,
                                };
                                let expected = matches!(
                                    (
                                        self_target,
                                        current_launch,
                                        anchors_team,
                                        caller_team,
                                        target_team,
                                        live_team,
                                        destination_allowed
                                    ),
                                    (false, true, true, Some("own"), Some("own"), true, true)
                                );
                                assert_eq!(tab_mutation_allowed(facts), expected);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn service_authority_resolves_live_teams_and_preserves_host_access() {
    let (mut mux, initial) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&initial).unwrap();
    let owner = mux.create_tab(&workspace).unwrap();
    let foreign = mux.create_tab(&workspace).unwrap();
    let target = mux.create_tab(&workspace).unwrap();
    for tab_id in [&owner, &foreign] {
        mux.set_orchestrator_role(tab_id).unwrap();
        mux.resolve_or_mint_team_for_parent(tab_id).unwrap();
    }
    mux.set_tab_role(&target, AgentRole::Worker, Some(&owner))
        .unwrap();
    mux.get_tab_mut(&owner).unwrap().session_binding_nonce = Some("current".into());
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let mut caller = TabCaller {
        tab_id: owner.clone(),
        binding_nonce: "current".into(),
    };
    assert!(service
        .authorize_tab_mutation(TabMutationCaller::Tab(&caller), &target, Default::default())
        .is_ok());
    for denied in [
        owner.as_str(),
        foreign.as_str(),
        initial.as_str(),
        "missing",
    ] {
        assert!(service
            .authorize_tab_mutation(TabMutationCaller::Tab(&caller), denied, Default::default())
            .is_err());
        assert!(service
            .authorize_tab_mutation(TabMutationCaller::Host, denied, Default::default())
            .is_ok());
    }
    for scope in [
        TabMutationScope {
            parent_tab_id: Some(&foreign),
            ..Default::default()
        },
        TabMutationScope {
            plan_id: Some("missing"),
            ..Default::default()
        },
        TabMutationScope {
            creates_team: true,
            ..Default::default()
        },
    ] {
        assert!(service
            .authorize_tab_mutation(TabMutationCaller::Tab(&caller), &target, scope)
            .is_err());
        assert!(service
            .authorize_tab_mutation(TabMutationCaller::Host, &target, scope)
            .is_ok());
    }
    caller.binding_nonce = "stale".into();
    assert!(service
        .authorize_tab_mutation(TabMutationCaller::Tab(&caller), &target, Default::default())
        .is_err());
}
