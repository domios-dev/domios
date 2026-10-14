use super::plans::{CreatePlanBody, ListPlansQuery};
use super::*;

#[test]
fn groups_from_csv_parses_key_label() {
    let g = groups_from_csv("phase-1:Phase 1,phase-2:Phase 2,solo");
    assert_eq!(g.len(), 3);
    assert_eq!(g[0].key, "phase-1");
    assert_eq!(g[0].label, "Phase 1");
    assert_eq!(g[2].key, "solo");
    assert_eq!(g[2].label, "solo");
}

#[test]
fn full_and_unowned_query_accept_1_and_true() {
    use axum::extract::Query;
    for raw in ["/?full=1", "/?full=true"] {
        let uri: axum::http::Uri = raw.parse().unwrap();
        let q: Query<ListPlansQuery> = Query::try_from_uri(&uri).unwrap();
        assert_eq!(q.full, Some(true), "full from {raw}");
    }
    let uri: axum::http::Uri = "/?unowned=1".parse().unwrap();
    let q: Query<ListPlansQuery> = Query::try_from_uri(&uri).unwrap();
    assert_eq!(q.unowned, Some(true));
}

#[test]
fn kinds_from_csv_splits() {
    let k = kinds_from_csv("status_update,testing,review");
    assert_eq!(k.len(), 3);
    assert_eq!(k[0].key, "status_update");
}

#[test]
fn tag_facets_from_body_accepts_json_array() {
    let facets = tag_facets_from_body(&serde_json::json!([
        {
            "key": "area",
            "label": "Area",
            "color_lane": true,
            "values": [
                { "v": "api", "label": "API", "color": "#38bdf8" },
                { "v": "ui", "label": "UI" }
            ]
        }
    ]))
    .unwrap();
    assert_eq!(facets.len(), 1);
    assert_eq!(facets[0].key, "area");
    assert!(facets[0].color_lane);
    assert_eq!(facets[0].values[0].v, "api");
    assert_eq!(facets[0].values[0].color.as_deref(), Some("#38bdf8"));
}

#[test]
fn tag_facets_from_body_accepts_compact_string() {
    let facets =
        tag_facets_from_body(&serde_json::json!("area:Area:api=API#38bdf8|ui=UI#34d399")).unwrap();
    assert_eq!(facets.len(), 1);
    assert_eq!(facets[0].label.as_deref(), Some("Area"));
    assert_eq!(facets[0].values.len(), 2);
    assert_eq!(facets[0].values[1].label.as_deref(), Some("UI"));
}

#[test]
fn create_body_deserializes() {
    let body: CreatePlanBody = serde_json::from_str(r#"{"title":"T","kinds":"a,b"}"#).unwrap();
    assert_eq!(body.title, "T");
    assert_eq!(body.kinds.as_deref(), Some("a,b"));
}

#[test]
fn optional_string_field_preserves_missing_vs_null() {
    let body = serde_json::json!({
        "model": "opus",
        "why": null
    });

    assert_eq!(optional_string_field(&body, "missing"), None);
    assert_eq!(optional_string_field(&body, "why"), Some(None));
    assert_eq!(optional_string_field(&body, "model"), Some(Some("opus")));
}

#[test]
fn http_binding_pending_error_preserves_type_and_retryability() {
    let (status, axum::Json(body)) =
        plan_authority_err(crate::service::PlanAuthorityError::SessionBindingPending);

    assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["type"], "session_binding_pending");
    assert_eq!(body["retryable"], true);

    let (status, axum::Json(body)) =
        plan_authority_err(crate::service::PlanAuthorityError::SessionUnregistered);

    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
    assert_eq!(body["type"], "plan_binding_session_unregistered");
    assert_eq!(body["retryable"], false);
}
