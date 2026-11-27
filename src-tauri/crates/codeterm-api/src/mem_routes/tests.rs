use super::*;
use axum::{extract::State, http::HeaderValue, Json};
use codeterm_neighbors_service::NeighborsFixtureExt;
use codeterm_service_core::multiplexer::{state::MultiplexerState, types::NeighborAuthority};

#[tokio::test]
async fn actor_and_selected_tab_headers_cannot_forge_save_author() {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let friend = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&friend).unwrap().neighbor_authority = Some(NeighborAuthority {
        uid: "friend".into(),
        friendship_id: "friendship".into(),
        root_tab_id: None,
    });
    let fixture = codeterm_neighbors_service::test_support::fixture(mux);
    fixture.seed_neighbor_author("friend", "friendship", "remote-friend");
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());
    state.config.lock().memory.auto_extract = false;
    let owner_token = state
        .svc
        .frontend_pty_env(Some(&owner))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let friend_token = state
        .svc
        .frontend_pty_env(Some(&friend))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {owner_token}")).unwrap(),
    );
    headers.insert(
        "x-codeterm-actor",
        HeaderValue::from_static("neighbor:forged"),
    );
    headers.insert("x-codeterm-tab-id", HeaderValue::from_str(&friend).unwrap());
    let body = serde_json::from_value(serde_json::json!({"kind":"fact","title":"Authored fact","body":"a complete body","skip_extract":true})).unwrap();
    let Json(saved) = items::mem_save(
        State(state.clone()),
        crate::auth::AgentHeaders::for_owner_test(headers.clone()),
        Json(body),
    )
    .await
    .unwrap();
    let stored = state
        .svc
        .mem
        .get(saved["id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.author.kind,
        codeterm_mem::store::MemAuthorKind::Owner
    );
    assert_eq!(
        serde_json::to_value(&stored).unwrap()["author"]["kind"],
        "owner"
    );
    let history = state.svc.mem.get_item_history(&stored.id).unwrap();
    assert_eq!(history[0].author, stored.author);
    headers.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {friend_token}")).unwrap(),
    );
    headers.insert("x-codeterm-actor", HeaderValue::from_static("owner"));
    assert_eq!(
        memory_author(&state, &headers).await.unwrap(),
        codeterm_mem::store::MemAuthor::neighbor("remote-friend".into(), "friendship".into())
    );
}

#[tokio::test]
async fn nonlocal_memory_actors_can_mutate_without_local_run_attribution() {
    use crate::auth::AuthenticatedOrigin;
    use codeterm_service_core::mesh::crown::{
        generate_peer_keypair, sign_request_with_agent, AuthenticatedAgent,
    };
    for origin in [
        AuthenticatedOrigin::Agent,
        AuthenticatedOrigin::Mesh,
        AuthenticatedOrigin::Service,
    ] {
        let (mux, _) = MultiplexerState::new();
        let fixture = codeterm_neighbors_service::test_support::fixture(mux);
        let state = crate::test_support::app_state(&fixture);
        state.config.lock().api.token = Some("owner-secret".into());
        let (private, public) = generate_peer_keypair();
        {
            let mut mesh = state.mesh.lock();
            mesh.ident.identity.instance_id = "remote-source".into();
            mesh.ident.identity.private_key = Some(private.clone());
            mesh.ident.identity.public_key = Some(public);
        }
        let agent = AuthenticatedAgent {
            instance_id: "remote-source".into(),
            tab_id: "remote-tab".into(),
            can_spawn: false,
        };
        let token = sign_request_with_agent(
            &private,
            "remote-source",
            origin,
            (origin == AuthenticatedOrigin::Agent).then_some(&agent),
        )
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
        headers.insert("x-codeterm-tab-id", "selected-local-tab".parse().unwrap());
        state.svc.workers.register(codeterm_mem::workers::make_info(
            "gw_unrelated".into(),
            "selected-local-tab".into(),
            "dreaming:rem".into(),
            "test".into(),
            "blue".into(),
        ));
        let new_item = |title: &str| codeterm_mem::NewMemItem {
            kind: codeterm_mem::MemKind::Note,
            scope: codeterm_mem::MemScope::Global,
            title: title.into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "remote mutation proof".into(),
            source: None,
            importance: 2.0,
            source_path: None,
            pinned: false,
        };
        let keeper = state.svc.mem.save(new_item("keeper")).unwrap();
        let lesser = state.svc.mem.save(new_item("lesser")).unwrap();
        let archived = state.svc.mem.save(new_item("archived")).unwrap();
        let Json(relation) = graph::mem_graph_relate(
            State(state.clone()),
            headers.clone(),
            Json(
                serde_json::from_value(
                    serde_json::json!({"from":keeper,"rel":"about","to":lesser}),
                )
                .unwrap(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(relation["ok"], true);
        let Json(update) = items::mem_update(
            State(state.clone()),
            headers.clone(),
            axum::extract::Path(keeper.clone()),
            Json(serde_json::from_value(serde_json::json!({"importance":4.0})).unwrap()),
        )
        .await
        .unwrap();
        assert_eq!(update["updated"].as_str(), Some(keeper.as_str()));
        let Json(archive) = items::mem_archive(
            State(state.clone()),
            headers.clone(),
            axum::extract::Path(archived.clone()),
            Json(serde_json::from_value(serde_json::json!({"reason":"regression proof"})).unwrap()),
        )
        .await
        .unwrap();
        assert_eq!(archive["archived"].as_str(), Some(archived.as_str()));
        let Json(merge) = mem_merge::mem_merge(
            State(state.clone()),
            headers.clone(),
            Json(
                serde_json::from_value(serde_json::json!({"keeper":keeper,"lesser":lesser}))
                    .unwrap(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(merge["keeper"].as_str(), Some(keeper.as_str()));
        assert_eq!(state.svc.mem.get(&keeper).unwrap().unwrap().importance, 4.0);
        assert_eq!(
            state.svc.mem.get(&archived).unwrap().unwrap().kind,
            "archived"
        );
        assert_eq!(
            state.svc.mem.get(&lesser).unwrap().unwrap().kind,
            "archived"
        );
        assert!(state
            .svc
            .workers
            .get("gw_unrelated")
            .unwrap()
            .touched_nodes
            .is_empty());
    }
}
