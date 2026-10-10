use super::*;

#[test]
fn model_identifier_splits_at_the_provider_boundary() {
    assert_eq!(
        parse_model_id(" provider/model/variant "),
        Some(ModelId {
            provider_id: "provider".into(),
            model_id: "model/variant".into(),
        })
    );
    assert!(parse_model_id("provider/").is_none());
    assert!(parse_model_id("/model").is_none());
    assert!(parse_model_id("model").is_none());
}

#[test]
fn openapi_probe_requires_the_session_switch_post_route() {
    assert!(model_switch_route_available(&json!({
        "paths": {
            "/api/session/{sessionID}/model": {"post": {"responses": {"204": {}}}}
        }
    })));
    assert!(!model_switch_route_available(&json!({
        "paths": {
            "/api/session/{sessionID}/model": {"get": {"responses": {"200": {}}}}
        }
    })));
    assert!(!model_switch_route_available(&json!({
        "paths": {
            "/session/{sessionID}/model": {"post": {}}
        }
    })));
}

#[test]
fn session_switch_request_uses_v2_model_reference_shape() {
    let model = parse_model_id("provider/model-next").unwrap();
    assert_eq!(
        model_switch_body(&model, None),
        json!({
            "model": {
                "id": "model-next",
                "providerID": "provider"
            }
        })
    );
}

#[test]
fn live_model_switch_variant_is_inside_the_model_reference() {
    let model = parse_model_id("fixture/model").unwrap();
    assert_eq!(
        model_switch_body(&model, Some("high")),
        json!({
            "model": { "providerID": "fixture", "id": "model", "variant": "high" }
        })
    );
}

#[test]
fn confirmation_parser_reads_only_the_returned_session_model() {
    assert_eq!(
        parse_session_model(
            "ses_current",
            &json!({
                "id": "ses_current",
                "model": {"providerID": "provider", "id": "model-next"}
            })
        ),
        Some("provider/model-next".into())
    );
    assert_eq!(
        parse_session_model(
            "ses_current",
            &json!({
                "id": "ses_current",
                "model": {"providerID": "provider", "modelID": "model-next"}
            })
        ),
        Some("provider/model-next".into())
    );
    assert_eq!(
        parse_session_model("ses_current", &json!({"id": "ses_current"})),
        None
    );
    assert_eq!(
        parse_session_model(
            "ses_current",
            &json!({
                "id": "ses_other",
                "model": {"providerID": "provider", "id": "model-next"}
            })
        ),
        None
    );
}

#[test]
fn request_urls_keep_the_bound_session_and_directory() {
    let url = endpoint_url(
        "http://127.0.0.1:4096",
        &["api", "session", "ses_a/b", "model"],
        Some("D:/work/project"),
    )
    .unwrap();
    assert_eq!(url.path(), "/api/session/ses_a%2Fb/model");
    assert_eq!(
        url.query_pairs()
            .find(|(key, _)| key == "directory")
            .unwrap()
            .1,
        "D:/work/project"
    );
}
