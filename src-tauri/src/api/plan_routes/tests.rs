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
fn create_body_deserializes() {
    let body: CreatePlanBody = serde_json::from_str(r#"{"title":"T","kinds":"a,b"}"#).unwrap();
    assert_eq!(body.title, "T");
    assert_eq!(body.kinds.as_deref(), Some("a,b"));
}
