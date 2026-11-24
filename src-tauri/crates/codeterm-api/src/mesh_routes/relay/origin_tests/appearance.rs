use super::*;
use crate::wallpaper_routes::{self, WallpaperAssets};
use codeterm_mesh::crown::AppearanceStoreScope;
use codeterm_service_core::{
    config::schema::{AppConfig, AppearanceLook},
    service::{chat_wallpaper::WallpaperStore, general_agent},
};

fn proposal_command() -> RelayCommand {
    let toml = concat!(
        "[theme]\n",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../themes/appearance/forest-night.toml"
        ))
    );
    RelayCommand {
        method: "POST".into(),
        path: "/api/appearance/target".into(),
        body: json!({"toml": toml, "rationale": "Forest palette"}),
        delivery_id: None,
    }
}

fn bound_token(
    source: &AppState,
    credential: &str,
    destination: &str,
    cmd: &RelayCommand,
) -> Result<String, (StatusCode, String)> {
    let scope = AppearanceStoreScope::new(destination, &cmd.method, &cmd.path, &cmd.body).unwrap();
    origin::signed_token_for_local_caller(source, &headers(credential), Some(scope))
        .map(|(_, token)| token)
}

fn relay_request(token: &str, cmd: &RelayCommand) -> Request<Body> {
    let client = PeerClient::new_signed("127.0.0.1", 1, token);
    wire_request(client.relay_request(cmd).unwrap())
}

async fn call(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 65536).await.unwrap();
    let body = serde_json::from_slice(&body)
        .unwrap_or_else(|_| Value::String(String::from_utf8(body.to_vec()).unwrap()));
    (status, body)
}

fn target_router(state: &AppState, assets: &std::sync::Arc<WallpaperAssets>) -> Router {
    wallpaper_routes::routes(assets.clone())
        .route(
            "/api/mesh/relay",
            post(crate::mesh_routes::mesh_relay).layer(axum::Extension(assets.clone())),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state.clone())
}

fn signed_source_tabs() -> (ServiceFixture, String, String) {
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
    let fixture = ServiceFixture::new(mux);
    let token = |id: &str| {
        fixture
            .service
            .frontend_pty_env(Some(id))
            .unwrap()
            .remove("CODETERM_API_TOKEN")
            .unwrap()
    };
    let (main, fresh) = (token(&main), token(&fresh));
    (fixture, main, fresh)
}

#[tokio::test]
async fn appearance_relay_binds_destination_operation_and_body_and_refuses_replay() {
    let (source_fixture, main, fresh) = signed_source_tabs();
    let target_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let source = state(&source_fixture, "source");
    let target = state(&target_fixture, "target");
    let other = state(&target_fixture, "other");
    pair(&target, &source);
    pair(&other, &source);
    let config_path = target_fixture.temp.path().join("appearance-test.toml");
    let assets = WallpaperAssets::new(
        WallpaperStore::new(target_fixture.temp.path().to_owned()),
        Ok(config_path.clone()),
    );
    let app = target_router(&target, &assets);
    let other_app = target_router(&other, &assets);
    let original = AppearanceLook::from_config(&target.config.lock());
    let cmd = proposal_command();
    assert_eq!(
        bound_token(&source, &fresh, "target", &cmd).unwrap_err().0,
        StatusCode::FORBIDDEN
    );
    let scoped = bound_token(&source, &main, "target", &cmd).unwrap();

    let (status, _) = call(&other_app, relay_request(&scoped, &cmd)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let mut substituted = cmd.clone();
    substituted.body["rationale"] = json!("Substituted");
    let (status, _) = call(&app, relay_request(&scoped, &substituted)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let mut substituted = cmd.clone();
    substituted.path.push_str("/record/apply");
    let (status, _) = call(&app, relay_request(&scoped, &substituted)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let mut substituted = cmd.clone();
    substituted.method = "GET".into();
    let (status, _) = call(&app, relay_request(&scoped, &substituted)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let direct_request = |token: &str, cmd: &RelayCommand| {
        wire_request(
            origin::loopback_request_with_auth(
                &target,
                cmd,
                &cmd.body,
                headers(token)["authorization"].clone(),
            )
            .unwrap(),
        )
    };
    let (status, _) = call(&app, direct_request(&scoped, &cmd)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (_, ordinary_agent) =
        origin::signed_token_for_local_caller(&source, &headers(&main), None).unwrap();
    let (status, _) = call(&app, direct_request(&ordinary_agent, &cmd)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    assert!(target
        .mesh
        .lock()
        .set_peer_access("source", AccessLevel::ReadOnly));
    let (status, error) = call(&app, relay_request(&scoped, &cmd)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{error}");
    assert!(error.as_str().unwrap().contains("Full mesh access"));
    assert!(!config_path.exists());
    assert!(target
        .mesh
        .lock()
        .set_peer_access("source", AccessLevel::Full));

    let (status, response) = call(&app, relay_request(&scoped, &cmd)).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["status"], 200, "{response}");
    let proposal = &response["data"];
    assert_eq!(proposal["state"], "proposed");
    assert_eq!(proposal["machine"]["id"], "target");
    assert_eq!(AppearanceLook::from_config(&target.config.lock()), original);

    let (status, _) = call(&app, relay_request(&scoped, &cmd)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let id = proposal["id"].as_str().unwrap();
    let persisted: AppConfig =
        toml::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    assert_eq!(persisted.appearance_proposals.len(), 1);
    assert_eq!(persisted.appearance_proposals[0].id, id);
    assert_eq!(
        persisted.appearance_proposals[0]
            .proposed_by_installation
            .as_deref(),
        Some("source")
    );
    assert_eq!(AppearanceLook::from_config(&persisted), original);

    let (_, owner) =
        origin::signed_token_for_local_caller(&source, &headers("owner-secret"), None).unwrap();
    for (action, expected) in [("apply", "applied"), ("undo", "undone")] {
        let cmd = RelayCommand {
            method: "POST".into(),
            path: format!("{}/{id}/{action}", cmd.path),
            body: Value::Null,
            delivery_id: None,
        };
        let (status, _) = call(&app, direct_request(&ordinary_agent, &cmd)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, receipt) = call(&app, direct_request(&owner, &cmd)).await;
        assert_eq!(status, StatusCode::OK, "{receipt}");
        assert_eq!(receipt["state"], expected);
        assert_eq!(
            AppearanceLook::from_config(&target.config.lock()) == original,
            action == "undo"
        );
    }
}

#[test]
fn appearance_relay_refuses_stale_grant_at_persistence() {
    let (source_fixture, main, _) = signed_source_tabs();
    let target_fixture = ServiceFixture::new(MultiplexerState::new().0);
    let source = state(&source_fixture, "source");
    let target = state(&target_fixture, "target");
    pair(&target, &source);
    let cmd = proposal_command();
    let token = bound_token(&source, &main, "target", &cmd).unwrap();
    let origin::RelayAdmission::AppearanceStore(admission) =
        origin::authenticate_relay(&target, &headers(&token), Some(&cmd)).unwrap()
    else {
        panic!("Expected a scoped proposal admission");
    };
    assert!(target
        .mesh
        .lock()
        .set_peer_access("source", AccessLevel::ReadOnly));
    assert!(target
        .mesh
        .lock()
        .set_peer_access("source", AccessLevel::Full));
    let path = target_fixture.temp.path().join("stale-appearance.toml");
    let assets = WallpaperAssets::new(
        WallpaperStore::new(target_fixture.temp.path().to_owned()),
        Ok(path.clone()),
    );
    let error = crate::appearance_routes::store_relay(&target, &assets, admission).unwrap_err();
    assert_eq!(error.0, StatusCode::FORBIDDEN);
    assert!(!path.exists());
    assert!(target.config.lock().appearance_proposals.is_empty());
}
