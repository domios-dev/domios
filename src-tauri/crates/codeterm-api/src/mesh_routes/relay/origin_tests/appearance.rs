use super::*;
use codeterm_service_core::{
    config::schema::AppearanceLook,
    service::{chat_wallpaper::WallpaperStore, general_agent},
};

async fn relay_appearance(
    app: &Router,
    source: &AppState,
    target: &AppState,
    token: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let cmd = RelayCommand {
        method: "POST".into(),
        path: path.into(),
        body,
        delivery_id: None,
    };
    let client = origin::peer_client(
        source,
        &headers(token),
        PeerClient::new_signed("127.0.0.1", 1, "discarded"),
    )
    .unwrap();
    let outward = client.relay_request(&cmd).unwrap();
    let (peer, _, _) = origin::authenticate_signed_mesh_request(target, outward.headers()).unwrap();
    assert_eq!(peer, "source");
    let request = origin::loopback_request_with_auth(
        target,
        &cmd,
        &cmd.body,
        outward.headers()["authorization"].clone(),
    )
    .unwrap();
    let response = app.clone().oneshot(wire_request(request)).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 65536).await.unwrap();
    let body = serde_json::from_slice(&body)
        .unwrap_or_else(|_| Value::String(String::from_utf8(body.to_vec()).unwrap()));
    (status, body)
}

#[tokio::test]
async fn appearance_proposal_survives_relay_and_only_owner_can_commit_it() {
    let (mut mux, main) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&main).unwrap();
    let fresh = mux.create_tab(&workspace).unwrap();
    mux.register_alias(general_agent::GENERAL_ALIAS, &main);
    for id in [&main, &fresh] {
        let tab = mux.get_tab_mut(id).unwrap();
        general_agent::identity::apply_general_agent_kind(tab);
        tab.protected = true;
        tab.agent_type = Some("fixture".into());
        tab.agent_session_id = Some(format!("session-{id}"));
        tab.session_binding_nonce = Some(format!("nonce-{id}"));
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    let source_fixture = ServiceFixture::new(mux);
    let target_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let source = state(&source_fixture, "source");
    let target = state(&target_fixture, "target");
    pair(&target, &source);
    let token = |id: &str| {
        source
            .svc
            .frontend_pty_env(Some(id))
            .unwrap()
            .remove("CODETERM_API_TOKEN")
            .unwrap()
    };
    let main_token = token(&main);
    let fresh_token = token(&fresh);
    let config_path = target_fixture.temp.path().join("appearance-test.toml");
    let app = crate::wallpaper_routes::with_store(
        WallpaperStore::new(target_fixture.temp.path().to_owned()),
        Ok(config_path.clone()),
    )
    .layer(axum::middleware::from_fn_with_state(
        target.clone(),
        crate::auth::auth_middleware,
    ))
    .with_state(target.clone());
    let original = AppearanceLook::from_config(&target.config.lock());
    let toml = concat!(
        "[theme]\n",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../themes/appearance/forest-night.toml"
        ))
    );
    let draft = json!({"toml": toml, "rationale": "Forest palette"});
    let path = "/api/appearance/target";

    let (status, error) =
        relay_appearance(&app, &source, &target, &fresh_token, path, draft.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{error}");
    assert!(target
        .mesh
        .lock()
        .set_peer_access("source", AccessLevel::ReadOnly));
    let (status, error) =
        relay_appearance(&app, &source, &target, &main_token, path, draft.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{error}");
    assert!(error.as_str().unwrap().contains("Full mesh access"));
    assert!(!config_path.exists());
    assert!(target
        .mesh
        .lock()
        .set_peer_access("source", AccessLevel::Full));

    let (status, proposal) =
        relay_appearance(&app, &source, &target, &main_token, path, draft).await;
    assert_eq!(status, StatusCode::OK, "{proposal}");
    assert_eq!(proposal["state"], "proposed");
    assert_eq!(proposal["machine"]["id"], "target");
    assert_eq!(AppearanceLook::from_config(&target.config.lock()), original);

    let id = proposal["id"].as_str().unwrap();
    let persisted: codeterm_service_core::config::schema::AppConfig =
        toml::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    assert_eq!(persisted.appearance_proposals[0].id, id);
    assert_eq!(AppearanceLook::from_config(&persisted), original);
    let apply = format!("{path}/{id}/apply");
    let undo = format!("{path}/{id}/undo");
    for action in [&apply, &undo] {
        let (status, error) =
            relay_appearance(&app, &source, &target, &main_token, action, Value::Null).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{error}");
    }

    let (status, applied) =
        relay_appearance(&app, &source, &target, "owner-secret", &apply, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["state"], "applied");
    assert_eq!(applied["can_undo"], true);
    assert_ne!(AppearanceLook::from_config(&target.config.lock()), original);

    let (status, undone) =
        relay_appearance(&app, &source, &target, "owner-secret", &undo, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{undone}");
    assert_eq!(undone["state"], "undone");
    assert_eq!(AppearanceLook::from_config(&target.config.lock()), original);
}
