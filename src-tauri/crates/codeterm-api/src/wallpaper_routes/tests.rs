use super::*;
use axum::{body::to_bytes, http::Request};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use tower::ServiceExt;

const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8">
  <path d="M0 0L8 8"/>
</svg>"#;

fn fixture_router(fixture: &ServiceFixture, state: AppState) -> Router {
    with_store(
        WallpaperStore::new(fixture.temp.path().to_owned()),
        Ok(fixture.temp.path().join("wallpaper-settings.toml")),
    )
    .layer(axum::middleware::from_fn_with_state(
        state.clone(),
        crate::auth::auth_middleware,
    ))
    .with_state(state)
}

async fn json_call(
    app: &Router,
    token: &str,
    method: &str,
    path: &str,
    value: serde_json::Value,
) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn body_json(response: Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1024).await.unwrap()).unwrap()
}

fn bind_general(mux: &mut MultiplexerState, tab: &str, primary: bool) -> String {
    let workspace = mux.find_workspace_for_tab(tab).unwrap();
    let general = mux.create_tab(&workspace).unwrap();
    if primary {
        mux.register_alias("general", &general);
    }
    let main = mux.get_tab_mut(&general).unwrap();
    main.agent_type = Some("fixture".into());
    main.agent_session_id = Some(format!("session-{general}"));
    main.session_binding_nonce = Some(format!("nonce-{general}"));
    main.reported_session_binding_nonce = main.session_binding_nonce.clone();
    main.protected = primary;
    main.role = Some(codeterm_service_core::multiplexer::types::AgentRole::Manager);
    main.role_profile =
        Some(codeterm_service_core::service::general_agent::GENERAL_AGENT_KIND.into());
    general
}

fn tab_token(state: &AppState, tab: &str) -> String {
    state
        .svc
        .frontend_pty_env(Some(tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap()
}

#[tokio::test]
async fn appearance_keeps_proposals_separate_from_the_applied_look_and_undoes_all_fields() {
    let (mut mux, tab) = MultiplexerState::new();
    let general = bind_general(&mut mux, &tab, true);
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    let owner = "owner-secret";
    state.config.lock().api.token = Some(owner.into());
    let unprivileged = tab_token(&state, &tab);
    let general = tab_token(&state, &general);
    let before = codeterm_types::config::schema::AppearanceLook::from_config(&state.config.lock());
    let app = fixture_router(&fixture, state.clone());
    let path = "/api/appearance/local";
    let mut draft = serde_json::to_value(&before).unwrap();
    draft["theme"]["name"] = json!("Quiet Space");
    draft["theme"]["chat_background"] = json!({"pattern": "svg", "svg": SVG, "scale": 32});
    draft["wallpaper"] = json!({"background": {"pattern":"space", "opacity":0.04}, "fill":{"mode":"gradient","preset":"midnight"}, "fill_strength":0.4});
    draft["tuning"] =
        json!({"roundness":0.0, "shadow_strength":0.5, "edges":"defined", "font_size":1});
    let proposed = json!({"look":draft, "rationale":"Analogous navy and cool blue"});
    let refused = json_call(&app, &unprivileged, "POST", path, proposed.clone()).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    let mut invalid = proposed.clone();
    invalid["look"]["theme"]["bright_black"] = invalid["look"]["theme"]["background"].clone();
    invalid["look"]["theme"]["foreground"] = invalid["look"]["theme"]["background"].clone();
    let refused = json_call(&app, owner, "POST", path, invalid).await;
    assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let bytes = to_bytes(refused.into_body(), 65536).await.unwrap();
    let refusal: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(refusal["code"], "appearance_contrast");
    assert!(refusal["failures"]
        .as_array()
        .unwrap()
        .iter()
        .any(|pair| pair["fix"]["field"] == "theme.foreground"));

    let response = json_call(&app, &general, "POST", path, proposed).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    let proposal: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let id = proposal["id"].as_str().unwrap();
    assert_eq!(proposal["state"], "proposed");
    assert_eq!(
        codeterm_types::config::schema::AppearanceLook::from_config(&state.config.lock()),
        before
    );

    let apply = format!("{path}/{id}/apply");
    let undo = format!("{path}/{id}/undo");
    let response = json_call(&app, owner, "POST", &apply, json!(null)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let settings = fixture.temp.path().join("wallpaper-settings.toml");
    let read = || -> codeterm_types::config::schema::AppConfig {
        toml::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap()
    };
    let applied = read();
    assert_eq!(applied.appearance_active.as_deref(), Some(id));
    assert_eq!(applied.appearance.unwrap().roundness, 0.0);
    assert_eq!(applied.theme.name, "Quiet Space");
    let paper = applied.chat_wallpaper.unwrap();
    assert_eq!(
        paper.background.pattern,
        codeterm_types::config::schema::ChatPattern::Space
    );
    assert_eq!(paper.fill_strength, Some(0.4));
    let use_theme = format!("{path}/{id}/use-theme-wallpaper");
    let response = json_call(&app, &unprivileged, "POST", &use_theme, json!(null)).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = json_call(&app, owner, "POST", &use_theme, json!(null)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let applied = read();
    assert!(applied.chat_wallpaper.is_none());
    assert_eq!(applied.appearance_active.as_deref(), Some(id));
    assert_eq!(
        applied.theme.chat_background.unwrap().pattern,
        codeterm_types::config::schema::ChatPattern::Svg
    );
    let response = json_call(&app, owner, "POST", &undo, json!(null)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        codeterm_types::config::schema::AppearanceLook::from_config(&read()),
        before
    );
}

async fn appearance_json(response: Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 256 * 1024).await.unwrap()).unwrap()
}

#[tokio::test]
async fn appearance_tweak_requires_authenticated_main_and_rejects_other_fields() {
    use codeterm_service_core::mesh::{
        crown::{generate_peer_keypair, sign_request_with_origin},
        types::{AccessLevel, PairedPeer, RELAY_PROTOCOL_VERSION},
    };
    use codeterm_service_core::multiplexer::types::AgentRole;

    let (mut mux, tab) = MultiplexerState::new();
    mux.get_tab_mut(&tab).unwrap().role = Some(AgentRole::Worker);
    let main = bind_general(&mut mux, &tab, true);
    let sibling = bind_general(&mut mux, &tab, false);
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    let owner = "owner-secret";
    state.config.lock().api.token = Some(owner.into());
    let (private_key, public_key) = generate_peer_keypair();
    state.mesh.lock().insert_account_peer_for_test(PairedPeer {
        instance_id: "peer".into(),
        instance_name: "Peer".into(),
        addresses: vec![],
        api_port: 0,
        public_key,
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::Full,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    });
    let peer = sign_request_with_origin(&private_key, "peer", AuthenticatedOrigin::Owner).unwrap();
    let general = tab_token(&state, &main);
    let app = fixture_router(&fixture, state.clone());
    let current = json_call(&app, &general, "GET", "/api/appearance/local", json!(null)).await;
    assert_eq!(current.status(), StatusCode::OK);
    let revision = appearance_json(current).await["revision"].clone();
    let path = "/api/appearance/local/tweak";
    let valid = json!({"revision": revision, "toml": "[appearance]\nfont_size = 1"});

    for caller in [
        owner.to_owned(),
        peer,
        tab_token(&state, &tab),
        tab_token(&state, &sibling),
    ] {
        let response = json_call(&app, &caller, "POST", path, valid.clone()).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            body_json(response).await["code"],
            "appearance_main_required"
        );
    }
    for toml in [
        "[theme]\nname = 'Another theme'",
        "[theme]\nblue = '#ffffff'",
        "[chat_wallpaper.background]\npattern = 'dots'\nopacity = 0.05",
        "[chat_wallpaper.background]\nsvg = '<svg/>'",
        "[chat_wallpaper]\nasset = 'new.svg'",
        "[appearance]\nfont_size = 1\nunrecognized = true",
        "[chat_wallpaper.background]\nopacity = 0.05",
    ] {
        let request = json!({"revision": revision, "toml": toml});
        let response = json_call(&app, &general, "POST", path, request).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{toml}");
        let refusal = appearance_json(response).await;
        assert_eq!(refusal["code"], "appearance_proposal_required", "{toml}");
        assert!(refusal["message"]
            .as_str()
            .unwrap()
            .contains("domios appearance propose"));
    }
    let config = state.config.lock();
    assert!(config.appearance_proposals.is_empty());
    assert!(config.appearance.is_none());
    assert!(config.chat_wallpaper.is_none());
}

#[tokio::test]
async fn appearance_tweak_checks_contrast_and_revision_and_undoes_exactly() {
    use codeterm_types::config::schema::{AppConfig, AppearanceLook};

    let (mut mux, tab) = MultiplexerState::new();
    let main = bind_general(&mut mux, &tab, true);
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().chat_wallpaper = Some(
        serde_json::from_value(json!({
            "background": {
                "pattern": "dots", "opacity": 0.08,
                "gradient": {"kind": "linear", "stops": ["#000000", "#ffffff"]}
            },
            "fill_strength": 0.04
        }))
        .unwrap(),
    );
    let owner = "owner-secret";
    state.config.lock().api.token = Some(owner.into());
    let general = tab_token(&state, &main);
    let before = AppearanceLook::from_config(&state.config.lock());
    let app = fixture_router(&fixture, state.clone());
    let base = "/api/appearance/local";
    let path = format!("{base}/tweak");
    let current = json_call(&app, &general, "GET", base, json!(null)).await;
    assert_eq!(current.status(), StatusCode::OK);
    let revision = appearance_json(current).await["revision"].clone();

    let too_bright = json!({"revision": revision, "toml": "[chat_wallpaper]\nfill_strength = 1.0"});
    let refused = json_call(&app, &general, "POST", &path, too_bright).await;
    assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        appearance_json(refused).await["code"],
        "appearance_contrast"
    );
    let settings = fixture.temp.path().join("wallpaper-settings.toml");
    assert!(!settings.exists());
    assert_eq!(AppearanceLook::from_config(&state.config.lock()), before);

    let request = json!({"revision": revision, "toml": r#"
[appearance]
roundness = 0.4
shadow_strength = 0.5
edges = "defined"
font_size = 1

[chat_wallpaper]
fill_strength = 0.02

[chat_wallpaper.background]
opacity = 0.05
"#});
    let response = json_call(&app, &general, "POST", &path, request.clone()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let applied = appearance_json(response).await;
    assert_eq!(applied["state"], "applied");
    assert_eq!(applied["tweak"], true);
    assert_eq!(applied["can_undo"], true);
    assert_eq!(applied["previous"], serde_json::to_value(&before).unwrap());
    let mut expected = serde_json::to_value(&before).unwrap();
    expected["tuning"] =
        json!({"roundness": 0.4, "shadow_strength": 0.5, "edges": "defined", "font_size": 1});
    expected["wallpaper"]["fill_strength"] = json!(0.02);
    expected["wallpaper"]["background"]["opacity"] = json!(0.05);
    assert_eq!(applied["look"], expected);

    let id = applied["id"].as_str().unwrap();
    let read =
        || -> AppConfig { toml::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap() };
    let saved = read();
    assert_eq!(saved.appearance_active.as_deref(), Some(id));
    assert_eq!(
        serde_json::to_value(AppearanceLook::from_config(&saved)).unwrap(),
        expected
    );
    let stale = json_call(&app, &general, "POST", &path, request).await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    assert_eq!(body_json(stale).await["code"], "appearance_changed");
    assert_eq!(read().appearance_proposals.len(), 1);

    let undo = format!("{base}/{id}/undo");
    let undone = json_call(&app, owner, "POST", &undo, json!(null)).await;
    assert_eq!(undone.status(), StatusCode::OK);
    assert_eq!(appearance_json(undone).await["state"], "undone");
    let restored = read();
    assert_eq!(restored.appearance_active, None);
    assert_eq!(AppearanceLook::from_config(&restored), before);
    assert_eq!(AppearanceLook::from_config(&state.config.lock()), before);
}

#[tokio::test]
async fn chat_wallpaper_agent_write_refusals_point_to_the_native_proposal() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    let token = state
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let app = fixture_router(&fixture, state.clone());
    for (method, path, body) in [
        ("POST", "/api/chat-wallpaper/apply-svg", json!({"svg": SVG})),
        ("PUT", "/api/chat-wallpaper", json!({"wallpaper": null})),
    ] {
        let response = json_call(&app, &token, method, path, body).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let error = body_json(response).await;
        assert_eq!(error["code"], "wallpaper_owner_required");
        let message = error["message"].as_str().unwrap();
        assert!(message.contains("domios appearance propose"));
        assert!(message.contains("<ds-appearance>"));
        assert!(!message.contains("Settings"));
    }
    assert!(state.config.lock().chat_wallpaper.is_none());
}

#[tokio::test]
async fn chat_wallpaper_assets_enforce_owner_authority_and_serve_only_committed_bytes() {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());
    let agent = state
        .svc
        .frontend_pty_env(Some(&tab))
        .unwrap()
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let app = fixture_router(&fixture, state);
    let send = |method, path: String, token: String, body: Body| {
        app.clone().oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "image/svg+xml")
                .header("x-codeterm-authenticated-origin", "Owner")
                .body(body)
                .unwrap(),
        )
    };
    let upload_path = "/api/chat-wallpaper/assets".to_string();
    let response = send("POST", upload_path.clone(), agent.clone(), Body::from(SVG))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = send(
        "POST",
        upload_path.clone(),
        "owner-secret".into(),
        Body::from(vec![b' '; 65537]),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let response = send(
        "POST",
        upload_path.clone(),
        "owner-secret".into(),
        Body::from(r#"<svg viewBox="0 0 8 8"><script/></svg>"#),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
        b"It contains a script. Remove all script elements and save the SVG again."
    );
    let response = send("POST", upload_path, "owner-secret".into(), Body::from(SVG))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let receipt = body_json(response).await;
    let id = receipt["id"].as_str().unwrap();
    let asset_path = format!("/api/chat-wallpaper/assets/{id}");
    let serve_path = format!("/api/serve-file?wallpaper={id}");
    for (method, path) in [("GET", &serve_path), ("DELETE", &asset_path)] {
        let response = send(method, path.clone(), agent.clone(), Body::empty())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = send(
        "GET",
        serve_path.clone(),
        "owner-secret".into(),
        Body::empty(),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/svg+xml");
    assert_eq!(
        response.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    assert_eq!(
        response.headers()[header::CONTENT_SECURITY_POLICY],
        "sandbox; default-src 'none'"
    );
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
        br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><path d="M0 0L8 8"/></svg>"#
    );
    let response = json_call(
        &app,
        "owner-secret",
        "PUT",
        "/api/chat-wallpaper",
        json!({
            "wallpaper": {"background": {"pattern": "svg"}, "asset": id }
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = send(
        "DELETE",
        asset_path.clone(),
        "owner-secret".into(),
        Body::empty(),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let response = json_call(
        &app,
        "owner-secret",
        "PUT",
        "/api/chat-wallpaper",
        json!({"wallpaper": null}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = send("DELETE", asset_path, "owner-secret".into(), Body::empty())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = send("GET", serve_path, "owner-secret".into(), Body::empty())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn chat_wallpaper_general_draft_is_bound_once_to_the_current_session_and_setting() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let other = mux.create_tab(&workspace).unwrap();
    for id in [&general, &other] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_type = Some("fixture".into());
        tab.agent_session_id = Some(format!("session-{id}"));
        tab.session_binding_nonce = Some(format!("nonce-{id}"));
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    mux.register_alias("general", &general);
    let tab = mux.get_tab_mut(&general).unwrap();
    tab.protected = true;
    tab.role = Some(codeterm_service_core::multiplexer::types::AgentRole::Manager);
    tab.role_profile =
        Some(codeterm_service_core::service::general_agent::GENERAL_AGENT_KIND.into());
    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    let owner = "owner-secret";
    state.config.lock().api.token = Some(owner.into());
    let token = |id| {
        state
            .svc
            .frontend_pty_env(Some(id))
            .unwrap()
            .remove("CODETERM_API_TOKEN")
            .unwrap()
    };
    let other = token(&other);
    let general = token(&general);
    let app = fixture_router(&fixture, state.clone());
    let drafts = "/api/chat-wallpaper/drafts";
    let response = json_call(&app, &general, "POST", drafts, json!({})).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = json_call(&app, owner, "POST", drafts, json!({})).await;
    assert_eq!(response.status(), StatusCode::OK);
    let receipt = body_json(response).await;
    let request = format!("{drafts}/{}", receipt["request"].as_str().unwrap());
    let valid = json!({ "svg": SVG });
    let response = json_call(&app, &other, "POST", &request, valid.clone()).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = json_call(
        &app,
        &general,
        "POST",
        &request,
        json!({"svg":"<svg><script/></svg>"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = json_call(&app, &general, "POST", &request, valid.clone()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let id = body_json(response).await["id"].as_str().unwrap().to_owned();
    let path = fixture.temp.path().join("wallpaper-settings.toml");
    let persisted: codeterm_types::config::schema::AppConfig =
        toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        persisted.chat_wallpaper.unwrap().asset.as_deref(),
        Some(id.as_str())
    );
    let response = json_call(&app, &general, "POST", &request, valid.clone()).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = json_call(&app, owner, "POST", drafts, json!({})).await;
    let receipt = body_json(response).await;
    let request = format!("{drafts}/{}", receipt["request"].as_str().unwrap());
    let response = json_call(
        &app,
        owner,
        "PUT",
        "/api/chat-wallpaper",
        json!({"wallpaper":null}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = json_call(&app, &general, "POST", &request, valid).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(state.config.lock().chat_wallpaper.is_none());
}

#[tokio::test]
async fn chat_wallpaper_copy_and_undo_compare_and_persist_the_complete_look() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::test_support::app_state(&fixture);
    let owner = "owner-secret";
    state.config.lock().api.token = Some(owner.into());
    let app = fixture_router(&fixture, state.clone());
    let path = "/api/chat-wallpaper/appearance";
    let response = json_call(&app, owner, "GET", path, json!(null)).await;
    let before = body_json(response).await;
    let mut unsafe_theme = before.clone();
    unsafe_theme["theme"]["name"] = json!("../invalid");
    let response = json_call(
        &app,
        owner,
        "PUT",
        path,
        json!({
            "mutating": unsafe_theme, "expected": before
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let mut copied = before.clone();
    copied["theme"]["foreground"] = json!("#abcdef");
    copied["wallpaper"] = json!({"background":{"pattern":"domios"}});
    let response = json_call(
        &app,
        owner,
        "PUT",
        path,
        json!({
            "mutating": copied, "expected": before
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let after = body_json(json_call(&app, owner, "GET", path, json!(null)).await).await;
    assert_eq!(after, copied);
    let response = json_call(
        &app,
        owner,
        "PUT",
        path,
        json!({
            "mutating": before, "expected": before
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(state.config.lock().theme.foreground, "#abcdef");
    let response = json_call(
        &app,
        owner,
        "PUT",
        path,
        json!({
            "mutating": before, "expected": copied
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let restored = body_json(json_call(&app, owner, "GET", path, json!(null)).await).await;
    assert_eq!(restored, before);
}
