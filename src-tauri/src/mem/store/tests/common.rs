use std::path::PathBuf;

use rusqlite::Connection;

use super::super::*;

pub(super) fn fresh() -> (Connection, PathBuf) {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    let tmp = std::env::temp_dir().join(format!("memstore-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&tmp).unwrap();
    (conn, tmp)
}

pub(super) fn mk_item(title: &str, body: &str) -> NewMemItem {
    NewMemItem {
        kind: MemKind::Learning,
        scope: MemScope::Global,
        title: title.into(),
        summary: None,
        area: Some("test".into()),
        tags: vec![],
        body: body.into(),
        source: None,
        importance: 2.0,
        source_path: None,
        pinned: false,
    }
}
