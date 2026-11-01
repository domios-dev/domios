use rusqlite::Connection;

use crate::search::{hybrid_rrf, SearchQuery};
use crate::store::{init_schema, insert, MemKind, MemScope, NewMemItem};

fn seed(conn: &Connection, blobs: &std::path::Path, kind: MemKind, title: &str, body: &str) {
    let item = NewMemItem {
        kind,
        scope: MemScope::Global,
        title: title.into(),
        summary: None,
        area: None,
        tags: vec![],
        body: body.into(),
        source: None,
        importance: 3.0,
        source_path: None,
        pinned: false,
    };
    insert(conn, &item, blobs).unwrap();
}

fn top(conn: &Connection, blobs: &std::path::Path, query: &str) -> String {
    let q = SearchQuery {
        query: query.into(),
        limit: Some(5),
        ..Default::default()
    };
    hybrid_rrf(conn, &q, blobs).unwrap()[0].item.title.clone()
}

#[test]
fn search_ranks_exact_keyword_first() {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    let blobs = std::env::temp_dir().join(format!("memlex-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&blobs).unwrap();

    seed(
        &conn,
        &blobs,
        MemKind::Fact,
        "NAS nightly backup",
        "restic nightly at 02:00 to Backblaze B2 bucket archivist-b2-night",
    );
    seed(
        &conn,
        &blobs,
        MemKind::Fact,
        "Laptop backup",
        "Time Machine weekly backup",
    );
    seed(
        &conn,
        &blobs,
        MemKind::Fact,
        "Phone photos",
        "Immich hourly photo upload",
    );
    seed(
        &conn,
        &blobs,
        MemKind::Skill,
        "codeterm ai terminal control",
        "run commands in a tab, set the tab name, run agents",
    );
    seed(
        &conn,
        &blobs,
        MemKind::Skill,
        "verify before claim",
        "run the check, read the name of the failing test, run it again",
    );

    assert_eq!(top(&conn, &blobs, "restic run"), "NAS nightly backup");
    assert_eq!(
        top(&conn, &blobs, "when does restic run"),
        "NAS nightly backup"
    );
    assert_eq!(top(&conn, &blobs, "bucket name"), "NAS nightly backup");
    assert_eq!(top(&conn, &blobs, "Laptop backup"), "Laptop backup");
    assert_eq!(
        top(&conn, &blobs, "verify before claim"),
        "verify before claim"
    );
}
