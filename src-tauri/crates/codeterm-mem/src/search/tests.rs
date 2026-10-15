use super::*;
use crate::store::{init_schema, insert, MemKind, MemScope, NewMemItem};
use rusqlite::Connection;
use std::path::PathBuf;

fn fresh() -> (Connection, PathBuf) {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    let tmp = std::env::temp_dir().join(format!("memsrch-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&tmp).unwrap();
    (conn, tmp)
}

#[test]
fn fts_basic_recall() {
    let (conn, blobs) = fresh();
    insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "home assistant".into(),
            summary: None,
            area: Some("home".into()),
            tags: vec!["automation".into()],
            body: "control HA entities, scenes, automations".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "deploy pipeline".into(),
            summary: None,
            area: Some("deploy".into()),
            tags: vec![],
            body: "uses ./build.sh build for production".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    let r = run(
        &conn,
        &SearchQuery {
            query: "home automation".into(),
            ..Default::default()
        },
        &blobs,
    )
    .unwrap();
    assert!(!r.is_empty());
    assert_eq!(r[0].item.id, "skill_home_assistant");
}

#[test]
fn canonical_issue_query_uses_conjunctive_terms() {
    assert_eq!(
        sanitize_fts_query("rollacode/codeterm-releases#18", true),
        "rollacode AND codeterm AND releases AND 18"
    );
    assert!(sanitize_fts_query("dirty worktree report gate", true).contains(" OR "));
}

#[test]
fn kind_filter() {
    let (conn, blobs) = fresh();
    insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "auth skill".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "authentication helpers".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "auth fact".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "authentication broken on staging".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    let r = run(
        &conn,
        &SearchQuery {
            query: "authentication".into(),
            kind: Some("fact".into()),
            ..Default::default()
        },
        &blobs,
    )
    .unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].item.kind, "fact");
}

#[test]
fn keyword_overlap_boosts() {
    let kw: HashSet<String> = ["postgres", "database"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let item = MemItem {
        id: "x".into(),
        uid: "u".into(),
        kind: "fact".into(),
        scope: "global".into(),
        title: "Postgres notes".into(),
        summary: None,
        area: None,
        tags: vec![],
        body: "Postgres database tuning".into(),
        source: None,
        importance: 3.0,
        created_at: "".into(),
        updated_at: "".into(),
        accessed_at: None,
        access_count: 0,
        recurrence_count: 0,
    };
    assert!((keyword_overlap(&kw, &item) - 1.0).abs() < 0.01);
}

#[test]
fn temporal_offset_parsing() {
    assert_eq!(parse_temporal_offset("what did I do yesterday?"), Some(1));
    assert_eq!(parse_temporal_offset("last week's deploy"), Some(7));
    assert_eq!(parse_temporal_offset("3 weeks ago"), Some(21));
    assert_eq!(parse_temporal_offset("2 months ago"), Some(60));
    assert_eq!(parse_temporal_offset("nothing temporal here"), None);
}

#[test]
fn default_mode_is_hybrid_rrf() {
    // Default behaviour must stay unchanged until benched.
    let cfg = SearchConfig::default();
    assert!(matches!(cfg.mode, SearchMode::HybridRrf));
    assert!((cfg.vector_rerank_weight - 0.30).abs() < 1e-9);
    assert_eq!(cfg.vector_rerank_pool_multiplier, 5);
}

/// When the `embeddings` feature is off, vector-first must degrade cleanly
/// (empty result) rather than panicking. This is the contract callers rely
/// on: they can set `mode = VectorFirstRerank` and fall back to HybridRrf
/// via `unwrap_or_default`.
#[cfg(not(feature = "embeddings"))]
#[test]
fn vector_first_returns_empty_without_embeddings() {
    let (conn, blobs) = fresh();
    insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "a".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "b".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    let cfg = SearchConfig {
        mode: SearchMode::VectorFirstRerank,
        ..SearchConfig::default()
    };
    let r = run_vector_first(
        &conn,
        &SearchQuery {
            query: "anything".into(),
            ..Default::default()
        },
        &blobs,
        &cfg,
    )
    .unwrap();
    assert!(r.is_empty());
}

#[test]
fn archived_items_excluded_from_search() {
    let (conn, blobs) = fresh();
    insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "active deployment fact".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "active deployment pipeline details".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    // Insert an archived item directly — bypasses the kind enum since 'archived' is a runtime value written by the sweep, not a MemKind variant.
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, importance,
                evidence_count, created_at, updated_at, accessed_at, tags_json,
                is_pinned_wakeup, body_inline)
             VALUES ('fact_archived_deployment', 'fact_archived_deployment', 'archived',
                'global', 'archived deployment fact', 2.0, 1,
                datetime('now','-100 days'), datetime('now','-100 days'),
                NULL, '[]', 0, 'archived deployment pipeline details')",
        [],
    )
    .unwrap();
    conn.execute(
            "INSERT INTO mem_fts (mem_id, title, summary, body, tags)
             VALUES ('fact_archived_deployment', 'archived deployment fact', '', 'archived deployment pipeline details', '')",
            [],
        ).unwrap();
    let hits = run(
        &conn,
        &SearchQuery {
            query: "deployment".into(),
            ..Default::default()
        },
        &blobs,
    )
    .unwrap();
    let ids: Vec<&str> = hits.iter().map(|h| h.item.id.as_str()).collect();
    assert!(
        ids.iter().any(|id| id.contains("active")),
        "active item must appear in search results"
    );
    assert!(
        !ids.contains(&"fact_archived_deployment"),
        "archived item must not appear in search results"
    );
}

/// FTS path must behave the same regardless of `mode` — the dispatch only
/// kicks in via `hybrid_rrf_with_config`. `run` directly hits FTS.
#[test]
fn fts_path_unchanged_by_mode_flag() {
    let (conn, blobs) = fresh();
    insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "home assistant".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "scenes and automations".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    let cfg_a = SearchConfig::default();
    let cfg_b = SearchConfig {
        mode: SearchMode::VectorFirstRerank,
        ..SearchConfig::default()
    };
    let q = SearchQuery {
        query: "home automation".into(),
        ..Default::default()
    };
    let a = run_with_config(&conn, &q, &blobs, &cfg_a).unwrap();
    let b = run_with_config(&conn, &q, &blobs, &cfg_b).unwrap();
    assert_eq!(a.len(), b.len());
    assert_eq!(a[0].item.id, b[0].item.id);
}
