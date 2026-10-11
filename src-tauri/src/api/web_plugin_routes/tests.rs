use super::*;

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
