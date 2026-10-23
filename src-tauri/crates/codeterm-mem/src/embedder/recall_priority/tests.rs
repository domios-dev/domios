use super::*;

#[test]
fn recall_priority_missing_semantics_preserves_keyword_hits_in_both_search_modes() {
    use crate::embedder::testing::{connection, note};
    use crate::search::{hybrid_rrf_with_embedding, SearchConfig, SearchMode, SearchQuery};
    let conn = connection();
    crate::store::init_schema(&conn).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let id = crate::store::insert(
        &conn,
        &note("priority fixture", "keyword needle"),
        dir.path(),
    )
    .unwrap();
    for mode in [SearchMode::HybridRrf, SearchMode::VectorFirstRerank] {
        let cfg = SearchConfig {
            mode,
            ..SearchConfig::default()
        };
        let query = SearchQuery {
            query: "needle".into(),
            ..SearchQuery::default()
        };
        let hits = hybrid_rrf_with_embedding(&conn, &query, dir.path(), &cfg, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].item.id, id);
    }
}

#[test]
fn recall_priority_budget_uses_injected_clock_and_response_port() {
    let (_, degraded) = with_recall_status(|| {
        let started = RECALL.with(|state| state.borrow().unwrap().started);
        let mut waits = Vec::new();
        let remaining = remaining_at(started + Duration::from_millis(400));
        assert_eq!(
            wait(remaining, |budget| {
                waits.push(budget);
                Some(42)
            }),
            Some(42)
        );
        assert_eq!(waits, vec![Duration::from_millis(100)]);
        let expired = remaining_at(started + QUERY_BUDGET);
        assert_eq!(
            record(wait::<u8>(expired, |_| panic!(
                "expired query reached provider"
            ))),
            None
        );
    });
    assert!(degraded);
    assert_eq!(remaining(), QUERY_BUDGET);
}

#[test]
fn recall_priority_timeout_degrades_and_scopes_do_not_leak() {
    let (_, degraded) = with_recall_status(|| {
        assert_eq!(
            record(wait::<u8>(QUERY_BUDGET, |budget| {
                assert_eq!(budget, QUERY_BUDGET);
                None
            })),
            None
        );
        assert!(!with_recall_status(|| record(Some(1))).1);
    });
    assert!(degraded);
    assert!(!with_recall_status(|| ()).1);
}
