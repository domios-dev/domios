use super::*;
use codeterm_service_core::agent::chat_types::StructuredChat;
use codeterm_service_core::agent::provider::ModelInfo;
use codeterm_service_core::chatbackend::{ChatBackendError, ChatBackendProvider};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

struct PairBackend;

impl ChatBackendProvider for PairBackend {
    fn id(&self) -> &str {
        "cwd-fixture"
    }

    fn spawnable(&self) -> bool {
        true
    }

    fn supports_session_config(&self) -> bool {
        true
    }

    fn open_session(&self, context: serde_json::Value) -> Result<String, ChatBackendError> {
        Ok(context["tabId"].as_str().unwrap().to_string())
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }

    fn poll(&self, _: &str, _: Option<&str>) -> Result<StructuredChat, ChatBackendError> {
        Ok(StructuredChat::default())
    }

    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

fn await_bound_sessions(service: &ServiceContext, events: &Receiver<()>, tab_ids: &[&str]) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let bound = {
            let mux = service.mux.read();
            tab_ids.iter().all(|id| {
                mux.get_tab(id).is_some_and(|tab| {
                    tab.agent_type_locked
                        && matches!(&tab.backing,
                            codeterm_service_core::multiplexer::types::TabBacking::ChatBackend {
                                session_id, ..
                            } if session_id.as_str() == *id)
                })
            })
        };
        if bound {
            return;
        }
        events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("pair backend sessions must bind before the failure deadline");
    }
}

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
    let (changed, events) = channel();
    let listener = fixture.ui.listen("mux-state-changed", move |_| {
        let _ = changed.send(());
    });
    fixture
        .service
        .chatbackend_registry
        .register("cwd-fixture", std::sync::Arc::new(PairBackend));
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
    await_bound_sessions(
        &fixture.service,
        &events,
        &[&pair.orchestrator_tab_id, pair.planner_tab_id.as_ref().expect("default pair seats a Planner")],
    );

    {
        let mux = fixture.service.mux.read();
        assert_eq!(
            mux.get_tab(&caller).unwrap().cwd.as_ref(),
            Some(&caller_cwd)
        );
        assert_ne!(caller_cwd, project);
        for tab_id in [&pair.orchestrator_tab_id, pair.planner_tab_id.as_ref().expect("default pair seats a Planner")] {
            assert_eq!(
                mux.find_workspace_for_tab(tab_id).as_ref(),
                Some(&workspace_id)
            );
            assert_eq!(mux.get_tab(tab_id).unwrap().cwd.as_ref(), Some(&project));
        }
        assert_eq!(
            mux.get_tab(pair.planner_tab_id.as_ref().expect("default pair seats a Planner"))
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
    await_bound_sessions(&fixture.service, &events, &[&override_spawn.tab_id]);
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
    fixture.ui.unlisten(listener);
    fixture.service.close_tab(&override_spawn.tab_id).unwrap();
    fixture.service.close_tab(pair.planner_tab_id.as_ref().expect("default pair seats a Planner")).unwrap();
    fixture
        .service
        .close_tab(&pair.orchestrator_tab_id)
        .unwrap();
}
