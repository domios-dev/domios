use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use codeterm_neighbors::adapter::AuthenticatedCall;
use std::sync::Arc;

struct Calls {
    requests: parking_lot::Mutex<Vec<Value>>,
    error: Option<Error>,
    later: Option<(ServiceContext, bool)>,
}

#[async_trait::async_trait]
impl AuthenticatedCall for Calls {
    async fn invoke(&self, route: &str, data: Value) -> Result<Value> {
        assert_eq!(route, "friendsSetPaused");
        self.requests.lock().push(data.clone());
        if let Some(error) = self.error {
            return Err(error);
        }
        if let Some((service, paused)) = &self.later {
            service.observe_neighbor_pause(
                "owner",
                data["friendship_id"].as_str().unwrap(),
                *paused,
            );
        }
        Ok(
            json!({"schema_version":1,"friendship_id":data["friendship_id"],"paused":data["paused"]}),
        )
    }
    async fn id_token(&self) -> Result<zeroize::Zeroizing<String>> {
        panic!("mock never authenticates over the network")
    }
}

fn fixture() -> (ServiceFixture, String, String) {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let tab = mux.create_tab(&workspace).unwrap();
    let friendship = "a".repeat(64);
    let agent = mux.get_tab_mut(&tab).unwrap();
    agent.agent_type = Some("fixture".into());
    agent.agent_session_id = Some("saved-session".into());
    agent.session_binding_nonce = Some("saved-nonce".into());
    agent.neighbor_authority = Some(crate::multiplexer::types::NeighborAuthority {
        uid: "owner".into(),
        friendship_id: friendship.clone(),
        root_tab_id: None,
    });
    (ServiceFixture::new(mux), tab, friendship)
}

#[tokio::test]
async fn pause_and_resume_persist_without_changing_permissions_binding_or_limits() {
    let (fixture, tab, friend) = fixture();
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let mut settings = Settings::default();
    settings.bindings.insert(friend.clone(), tab.clone());
    settings.permissions.insert(
        friend.clone(),
        serde_json::from_value(json!({"allowed":true,"scope":"opaque grant","allow_memory":false}))
            .unwrap(),
    );
    metadata.save_settings(settings.clone()).await.unwrap();
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let captured = events.clone();
    fixture.ui.listen("neighbor-friend-paused", move |value| {
        captured.lock().push(value.clone())
    });
    let calls = Arc::new(Calls {
        requests: Default::default(),
        error: None,
        later: None,
    });
    let transport = FirebaseTransport {
        calls: calls.clone(),
        project: "fixture".into(),
        uid: "owner".into(),
        installation_id: "own-machine".into(),
    };
    for paused in [true, true, false] {
        let result = service
            .set_neighbor_paused(
                &transport,
                &metadata,
                &friend,
                paused,
                service.neighbor_history_generation(),
            )
            .await
            .unwrap();
        assert_eq!(result["paused"], paused);
        let actual = metadata.settings().await.unwrap();
        assert_eq!(actual.paused.contains(&friend), paused);
        assert_eq!(actual.bindings, settings.bindings);
        assert_eq!(
            serde_json::to_value(actual.permissions).unwrap(),
            serde_json::to_value(&settings.permissions).unwrap()
        );
        let mux = service.mux.read();
        let agent = mux.get_tab(&tab).unwrap();
        assert_eq!(agent.frozen, paused);
        assert_eq!(agent.agent_session_id.as_deref(), Some("saved-session"));
        assert_eq!(agent.session_binding_nonce.as_deref(), Some("saved-nonce"));
    }
    assert_eq!(calls.requests.lock().len(), 3);
    assert_eq!(events.lock().len(), 2);
    assert!(metadata.settings().await.unwrap().paused.is_empty());
}

#[tokio::test]
async fn a_server_refusal_or_newer_pause_signal_cannot_be_overwritten_by_an_old_response() {
    for error in [None, Some(Error::SessionRevoked)] {
        let (fixture, tab, friend) = fixture();
        let service = &fixture.service;
        let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
        let calls = Arc::new(Calls {
            requests: Default::default(),
            error,
            later: Some((service.clone(), false)),
        });
        let transport = FirebaseTransport {
            calls,
            project: "fixture".into(),
            uid: "owner".into(),
            installation_id: "own-machine".into(),
        };
        let result = service
            .set_neighbor_paused(
                &transport,
                &metadata,
                &friend,
                true,
                service.neighbor_history_generation(),
            )
            .await;
        match error {
            Some(error) => assert_eq!(result, Err(error)),
            None => assert_eq!(result.unwrap()["paused"], false),
        }
        assert!(!service.neighbor_friend_paused(&friend));
        assert!(metadata.settings().await.unwrap().paused.is_empty());
        assert!(!service.mux.read().get_tab(&tab).unwrap().frozen);
    }
}

#[test]
fn older_list_snapshots_cannot_regress_a_pause_signal_even_when_the_signal_repeats_the_value() {
    let (fixture, _, friend) = fixture();
    let service = &fixture.service;
    let before = service.neighbor_cell().pauses.snapshot();
    service.observe_neighbor_pause("owner", &friend, true);
    service
        .reconcile_neighbor_pauses(
            "owner",
            &json!({"friends":[{"friendship_id":friend,"my_paused":false}]}),
            &before,
        )
        .unwrap();
    assert!(service.neighbor_friend_paused(&friend));
    let version = service.neighbor_cell().pauses.version(&friend);
    service.observe_neighbor_pause("owner", &friend, true);
    assert_eq!(
        service
            .neighbor_cell()
            .pauses
            .observe(&friend, false, Some(version)),
        None
    );
    service.observe_neighbor_pause("owner", &friend, false);
    assert!(!service.neighbor_friend_paused(&friend));
    service.neighbor_cell().pauses.forget(&friend);
    assert_eq!(
        service
            .neighbor_cell()
            .pauses
            .observe(&friend, true, Some(99)),
        None
    );
    assert_eq!(service.neighbor_cell().pauses.get(&friend), None);
}

#[test]
fn paused_new_runtime_commits_a_thread_before_freezing_and_resumes_the_same_binding() {
    use crate::service::agent_runtime::engine;
    let (fixture, tab, friend) = fixture();
    let service = &fixture.service;
    {
        let mut mux = service.mux.write();
        let target = mux.get_tab_mut(&tab).unwrap();
        target.agent_session_id = None;
        target.backing = crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id: "fixture".into(),
            session_id: String::new(),
        };
    }
    service.observe_neighbor_pause("owner", &friend, true);
    crate::async_runtime::block_on(service.pause_neighbor_tab(&tab, &friend)).unwrap();
    assert!(!service.mux.read().get_tab(&tab).unwrap().frozen);
    {
        let mut mux = service.mux.write();
        let target = mux.get_tab_mut(&tab).unwrap();
        target.agent_session_id = Some("committed-thread".into());
        target.reported_session_binding_nonce = target.session_binding_nonce.clone();
    }
    let prepared = service.mux.read().get_tab(&tab).unwrap().clone();
    let _runtime = engine::test_support::live_runtime(&prepared);
    engine::publish(service, &tab, &engine::for_tab(&tab).unwrap());
    assert!(service.mux.read().get_tab(&tab).unwrap().frozen);
    service.observe_neighbor_pause("owner", &friend, false);
    let mux = service.mux.read();
    let target = mux.get_tab(&tab).unwrap();
    assert!(!target.frozen);
    assert_eq!(target.agent_session_id.as_deref(), Some("committed-thread"));
    assert_eq!(target.session_binding_nonce.as_deref(), Some("saved-nonce"));
}
