use super::*;

#[test]
fn plugin_frame_policy_allows_inline_styles_without_relaxing_scripts_or_network() {
    let policy = plugin_frame_csp("fixture");
    let directives: std::collections::BTreeMap<_, _> = policy
        .split("; ")
        .map(|directive| directive.split_once(' ').unwrap())
        .collect();
    assert_eq!(directives["script-src"], "'nonce-fixture'");
    assert_eq!(directives["style-src"], "'unsafe-inline'");
    assert_eq!(directives["default-src"], "'none'");
    assert_eq!(directives["connect-src"], "'none'");
    assert_eq!(directives["base-uri"], "'none'");
    assert_eq!(directives["form-action"], "'none'");
    assert_eq!(directives["img-src"], "blob: data:");
    assert_eq!(directives["font-src"], "blob: data:");
    assert_eq!(directives.len(), 8);
}

#[tokio::test]
async fn plugin_ui_html_response_binds_bundle_and_bridge_to_fresh_nonce() {
    let source = b"<html><head><style nonce=\"__CT_NONCE__\">body{margin:0}</style></head><body><script nonce=\"__CT_NONCE__\" src=\"./app.js\"></script></body></html>";
    let first = plugin_ui_response("index.html", source.to_vec(), &PluginUiQuery::default());
    let second = plugin_ui_response("index.html", source.to_vec(), &PluginUiQuery::default());
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(
        first.headers()[header::CACHE_CONTROL],
        "no-store, must-revalidate"
    );
    assert_eq!(
        first.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    let policy = first.headers()[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap()
        .to_string();
    assert_ne!(
        first.headers()[header::CONTENT_SECURITY_POLICY],
        second.headers()[header::CONTENT_SECURITY_POLICY]
    );
    let nonce = policy
        .split("script-src 'nonce-")
        .nth(1)
        .unwrap()
        .split('\'')
        .next()
        .unwrap();
    assert_eq!(policy, plugin_frame_csp(nonce));
    let bytes = axum::body::to_bytes(first.into_body(), 65536)
        .await
        .unwrap();
    let html = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(!html.contains("__CT_NONCE__"));
    assert_eq!(html.matches(&format!("nonce=\"{nonce}\"")).count(), 3);
}

#[tokio::test]
async fn plugin_ui_asset_response_preserves_bytes_mime_and_private_cache() {
    for (path, mime) in [
        ("app.js", "text/javascript; charset=utf-8"),
        ("styles.css", "text/css; charset=utf-8"),
        ("icon.svg", "image/svg+xml"),
        ("font.woff2", "font/woff2"),
    ] {
        let response = plugin_ui_response(path, b"fixture".to_vec(), &PluginUiQuery::default());
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], mime);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, max-age=31536000, immutable"
        );
        assert!(!response
            .headers()
            .contains_key(header::CONTENT_SECURITY_POLICY));
        let bytes = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), b"fixture");
    }
}

#[test]
fn plugin_document_context_has_no_authentication_query() {
    let context: PluginUiQuery = serde_json::from_value(serde_json::json!({
        "surface": "board", "instance": "own", "tab": "tab-1", "reload": 2,
    }))
    .unwrap();
    assert_eq!(
        serde_json::to_value(context).unwrap(),
        serde_json::json!({
            "surface": "board", "instance": "own", "tab": "tab-1",
        })
    );
    assert!(
        serde_json::from_value::<PluginUiQuery>(serde_json::json!({ "token": "secret" })).is_err()
    );
}

#[tokio::test]
async fn plugin_document_preserves_sandbox_policy_and_surface_context() {
    let context: PluginUiQuery = serde_json::from_value(serde_json::json!({
        "surface": "</script>", "instance": "own", "tab": "tab-1",
    }))
    .unwrap();
    let response = html_response(b"<html><head></head><body></body></html>", &context);
    let policy = response.headers()[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap();
    assert!(policy.contains("connect-src 'none'"));
    assert!(policy.contains("img-src blob: data:"));
    let bytes = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    let html = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(html.contains("\\u003c/script\\u003e"));
    assert!(!html.contains("surface:\"</script>"));
}
