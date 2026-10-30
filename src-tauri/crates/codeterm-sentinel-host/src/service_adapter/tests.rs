use super::*;

#[test]
fn ease_blueprint_global_read_authority_follows_live_role_without_mutation_grants() {
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    let (mut mux, tab) = MultiplexerState::new();
    mux.get_tab_mut(&tab).unwrap().cwd = Some("/repo".into());
    let fixture = ServiceFixture::new(mux);
    let caller = fixture.service.sentinel_caller(&tab, None).unwrap();
    assert!(!fixture.service.sentinel_read_all_authority(&caller).unwrap());
    {
        let mut mux = fixture.service.mux.write();
        mux.set_orchestrator_role(&tab).unwrap();
        mux.resolve_or_mint_team_for_parent(&tab).unwrap();
    }
    let caller = fixture.service.sentinel_caller(&tab, None).unwrap();
    assert!(fixture.service.sentinel_read_all_authority(&caller).unwrap());
    {
        let mut mux = fixture.service.mux.write();
        codeterm_service_core::service::general_agent::identity::apply_general_agent_kind(mux.get_tab_mut(&tab).unwrap());
        mux.get_tab_mut(&tab).unwrap().team_id = None;
    }
    let caller = fixture.service.sentinel_caller(&tab, None).unwrap();
    assert!(fixture.service.sentinel_read_all_authority(&caller).unwrap());
    fixture.service.mux.write().get_tab_mut(&tab).unwrap().session_binding_nonce = Some("new-session".into());
    assert!(fixture.service.sentinel_read_all_authority(&caller).is_err());
}

#[test]
fn sentinel_creator_team_and_workspace_scope_all_read_and_stop_operations() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = crate::store::Store::new(
        dir.path().join("sentinels.json"),
        codeterm_service_core::cron::clock::MockClock::starting_at(1000),
    );
    let params = codeterm_sentinel::test_support::watch(
        serde_json::to_value::<Option<String>>(Some("service-state-watch".into())).unwrap(),
        serde_json::json!({"uses":"probe","with":serde_json::json!({"command":{"lit":serde_json::to_value::<String>("status".into()).unwrap()}}),"every":serde_json::json!({"lit":serde_json::to_value::<String>("10s".into()).unwrap()}),"paused":false,"session":false}),
        serde_json::json!({"with":serde_json::json!({"question":{"lit":serde_json::to_value::<String>("state".into()).unwrap()}}),"buckets":serde_json::to_value::<Vec<String>>(vec!["done".into()]).unwrap(),"matches":codeterm_sentinel::test_support::literal_params(serde_json::to_value::<std::collections::BTreeMap<String,String>>(Default::default()).unwrap()),"until":serde_json::to_value::<Option<String>>(None).unwrap(),"rearm_on_change":serde_json::to_value::<std::collections::BTreeSet<String>>(Default::default()).unwrap(),"escalation_timeout_ms":serde_json::to_value::<Option<u64>>(None).unwrap(),"model_classification":false}),
        serde_json::Value::Null,
        serde_json::to_value::<Option<String>>(None).unwrap(),
    );
    let job = store
        .create_document(
            params,
            crate::store::Owner {
                creator_tab_id: "creator".into(),
                team_id: Some("team".into()),
                workspace_id: "workspace".into(),
                cwd: "/tmp".into(),
            },
            true,
        )
        .unwrap();
    let caller = SentinelCaller {
        tab: "creator".into(),
        team: Some("team".into()),
        workspace: "workspace".into(),
        cwd: "/tmp".into(),
        nonce: None,
    };
    assert!(caller.owns(&job));
    let owner = SentinelCaller {
        tab: "orchestrator".into(),
        ..caller.clone()
    };
    assert!(owner.may_list(&job, true));
    let foreign_reader = SentinelCaller { team: Some("foreign".into()), workspace: "foreign".into(), ..owner.clone() };
    assert!(foreign_reader.may_inspect(&job, true, false));
    assert!(!foreign_reader.may_inspect(&job, false, false));
    assert!(!foreign_reader.owns(&job));
    assert!(!foreign_reader.can_amend(&job));
    assert!(!owner.may_list(&job, false));
    assert!(!SentinelCaller {
        team: Some("foreign".into()),
        ..owner.clone()
    }
    .may_list(&job, true));
    assert!(!SentinelCaller {
        workspace: "foreign".into(),
        ..owner
    }
    .may_list(&job, true));
    assert!(SentinelCaller {
        tab: "editor".into(),
        ..caller.clone()
    }
    .can_amend(&job));
    for changed in [
        SentinelCaller {
            tab: "other".into(),
            ..caller.clone()
        },
        SentinelCaller {
            team: None,
            ..caller.clone()
        },
        SentinelCaller {
            workspace: "elsewhere".into(),
            ..caller.clone()
        },
    ] {
        assert!(!changed.owns(&job));
    }
    assert!(!SentinelCaller {
        workspace: "elsewhere".into(),
        ..caller.clone()
    }
    .can_amend(&job));

    let solo = store
        .create_document(
            serde_json::json!(codeterm_sentinel::test_support::watch(serde_json::json!("solo-state-watch"),serde_json::json!({"uses":"probe","with":{"command":{"lit":"status"}},"every":{"lit":"10s"}}),serde_json::json!({"with":{"question":{"lit":"state"}},"buckets":["done"]}),serde_json::json!(null),serde_json::json!(null))),
            crate::store::Owner {
                creator_tab_id: "creator".into(),
                team_id: None,
                workspace_id: "workspace".into(),
                cwd: "/tmp".into(),
            },
            true,
        )
        .unwrap();
    let formed_team_creator = SentinelCaller {
        team: Some("formed-team".into()),
        ..caller
    };
    assert!(formed_team_creator.owns(&solo));
    assert!(formed_team_creator.can_amend(&solo));
    assert!(!SentinelCaller {
        tab: "worker".into(),
        ..formed_team_creator
    }
    .can_amend(&solo));
}
#[test]
fn sentinel_graph_tab_caller_keeps_current_nonce_and_creator_scope() {
    let (mut mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let ws = mux.snapshot().active_workspace_id.unwrap();
    let creator = mux.create_tab(&ws).unwrap();
    let tab = mux.get_tab_mut(&creator).unwrap();
    tab.cwd = Some("/tmp".into());
    tab.session_binding_nonce = Some("supplied-current-nonce".into());
    assert!(caller_from_mux(&mux, &creator, None).is_err());
    assert!(caller_from_mux(&mux, &creator, Some("stale-nonce")).is_err());
    let caller = caller_from_mux(&mux, &creator, Some("supplied-current-nonce")).unwrap();
    assert_eq!(caller.tab, creator);
    assert_eq!(caller.workspace, ws);
    assert_eq!(caller.team, None);
    assert_eq!(caller.nonce.as_deref(), Some("supplied-current-nonce"));
    assert!(caller_from_mux(&mux, "foreign", Some("supplied-current-nonce")).is_err());
}

#[test]
fn blueprint_pipeline_access_follows_tab_or_team_owner() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = crate::store::Store::new(
        dir.path().join("sentinels.json"),
        codeterm_service_core::cron::clock::MockClock::starting_at(1000),
    );
    let pipeline=store.create_document(serde_json::json!({"schema_version":2,"kind":"pipeline","name":"owned-pipeline","owner":"team","trigger":{"type":"schedule","every":{"lit":"10s"}},"steps":[{"id":"emit","run":{"lit":"printf ready"}}]}),crate::store::Owner{creator_tab_id:"creator".into(),team_id:Some("team".into()),workspace_id:"workspace".into(),cwd:"/tmp".into()},true).unwrap();
    let teammate = SentinelCaller {
        tab: "teammate".into(),
        team: Some("team".into()),
        workspace: "workspace".into(),
        cwd: "/tmp".into(),
        nonce: None,
    };
    assert!(teammate.owns(&pipeline));
    assert!(teammate.can_amend(&pipeline));
    assert!(!SentinelCaller {
        team: Some("other-team".into()),
        ..teammate.clone()
    }
    .owns(&pipeline));
}
