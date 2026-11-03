use super::*;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::spawn_capability::test_backend::SpawnableTestBackend;

#[test]
fn pair_members_inherit_workspace_cwd_from_a_different_caller_cwd() {
    assert_pair_cwd(false);
}

#[test]
fn pair_members_inherit_restored_workspace_cwd_from_a_different_caller_cwd() {
    assert_pair_cwd(true);
}

fn assert_pair_cwd(restored: bool) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let caller_cwd = root.path().join("sibling");
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(&caller_cwd).unwrap();
    let project = project.to_str().unwrap().to_string();
    let caller_cwd = caller_cwd.to_str().unwrap().to_string();
    let mut mux = MultiplexerState::empty();
    let workspace_id = mux
        .create_workspace("Project".into(), Some(project.clone()))
        .id;
    let caller = mux.create_tab(&workspace_id).unwrap();
    mux.get_tab_mut(&caller).unwrap().cwd = Some(caller_cwd.clone());
    if restored {
        mux = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
    }
    let fixture = crate::test_fixture::ServiceFixture::new(mux);
    fixture.service.chatbackend_registry.register(
        "cwd-fixture",
        std::sync::Arc::new(SpawnableTestBackend { id: "cwd-fixture" }),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let pair = runtime
        .block_on(
            fixture
                .service
                .create_orchestration_pair(OrchestrationPairParams {
                    orchestrator_agent: "cwd-fixture".into(),
                    planner_agent: "cwd-fixture".into(),
                    workspace_id: Some(workspace_id.clone()),
                    spawned_by_tab_id: Some(caller.clone()),
                    ..Default::default()
                }),
        )
        .expect("both pair members must launch");

    {
        let mux = fixture.service.mux.read();
        assert_eq!(
            mux.get_tab(&caller).unwrap().cwd.as_ref(),
            Some(&caller_cwd)
        );
        assert_ne!(caller_cwd, project);
        for tab_id in [&pair.orchestrator_tab_id, &pair.planner_tab_id] {
            assert_eq!(
                mux.find_workspace_for_tab(tab_id).as_ref(),
                Some(&workspace_id)
            );
            assert_eq!(mux.get_tab(tab_id).unwrap().cwd.as_ref(), Some(&project));
        }
        assert_eq!(
            mux.get_tab(&pair.planner_tab_id)
                .unwrap()
                .parent_tab_id
                .as_ref(),
            Some(&pair.orchestrator_tab_id)
        );
    }

    let override_spawn = runtime
        .block_on(fixture.service.spawn_agent(
            SpawnAgentParams {
                agent: Some("cwd-fixture".into()),
                workspace_id: Some(workspace_id),
                spawned_by_tab_id: Some(caller),
                cwd: Some(caller_cwd.clone()),
                ..Default::default()
            },
            None,
        ))
        .expect("an explicit cwd must override the workspace");
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&override_spawn.tab_id)
            .unwrap()
            .cwd
            .as_ref(),
        Some(&caller_cwd)
    );
}
