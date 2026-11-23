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
