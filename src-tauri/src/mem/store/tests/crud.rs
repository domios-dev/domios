use rusqlite::params;

use super::super::*;
use super::common::fresh;

#[test]
fn insert_and_get_inline() {
    let (conn, blobs) = fresh();
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "Test Fact".into(),
            summary: Some("a fact".into()),
            area: None,
            tags: vec!["a".into(), "b".into()],
            body: "short body".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    assert_eq!(id, "fact_test_fact");
    let got = get(&conn, &id, &blobs).unwrap().unwrap();
    assert_eq!(got.body, "short body");
    assert_eq!(got.tags, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn get_updates_accessed_at_and_access_count() {
    let (conn, blobs) = fresh();
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "touch me".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "body".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    // Sleep 1 ms so rfc3339 timestamps differ even on fast CI hardware.
    std::thread::sleep(std::time::Duration::from_millis(2));

    let got = get(&conn, &id, &blobs).unwrap().unwrap();
    let created = got.created_at.clone();
    let accessed = got
        .accessed_at
        .clone()
        .expect("accessed_at should be set after get");
    assert!(
        accessed > created,
        "accessed_at ({}) must be newer than created_at ({})",
        accessed,
        created
    );
    assert_eq!(got.access_count, 1, "first get bumps access_count to 1");

    // Second get bumps again.
    std::thread::sleep(std::time::Duration::from_millis(2));
    let got2 = get(&conn, &id, &blobs).unwrap().unwrap();
    assert_eq!(got2.access_count, 2);
    assert!(got2.accessed_at.unwrap() > accessed);
}

#[test]
fn insert_blob_when_body_large() {
    let (conn, blobs) = fresh();
    let big = "x".repeat(5000);
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Note,
            scope: MemScope::Global,
            title: "big".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: big.clone(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    let got = get(&conn, &id, &blobs).unwrap().unwrap();
    assert_eq!(got.body.len(), big.len());
}

#[test]
fn delete_removes_from_fts() {
    let (conn, blobs) = fresh();
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Note,
            scope: MemScope::Global,
            title: "to delete".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "x".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    assert!(delete(&conn, &id).unwrap());
    assert!(get(&conn, &id, &blobs).unwrap().is_none());
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_fts WHERE mem_id = ?1",
            params![&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn insert_with_file_prefix_source_path_populates_source_size_and_mtime() {
    let (conn, blobs) = fresh();
    let dir = std::env::temp_dir().join(format!("memstore-skill-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let skill_md = dir.join("SKILL.md");
    std::fs::write(&skill_md, "# Test skill\n\nbody content here").unwrap();

    let source_with_prefix = format!("file:{}", skill_md.display());
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "test prefix skill".into(),
            summary: None,
            area: None,
            tags: vec!["folder-skill".into()],
            body: "# Test skill\n\nbody content here".into(),
            source: Some(source_with_prefix.clone()),
            importance: 4.0,
            source_path: Some(source_with_prefix),
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    let (size, mtime): (Option<i64>, Option<String>) = conn
        .query_row(
            "SELECT source_size, source_mtime FROM mem_items WHERE id = ?1",
            params![&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let expected_size = std::fs::metadata(&skill_md).unwrap().len() as i64;
    assert_eq!(
        size,
        Some(expected_size),
        "source_size must be populated on insert when source_path uses file: prefix"
    );
    assert!(
        mtime.is_some(),
        "source_mtime must be populated on insert when source_path uses file: prefix"
    );
}

#[test]
fn get_bumps_updated_at_when_source_file_changes() {
    let (conn, blobs) = fresh();
    let dir = std::env::temp_dir().join(format!("memstore-getbump-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let skill_md = dir.join("SKILL.md");
    std::fs::write(&skill_md, "# v1\n\noriginal").unwrap();

    let source = format!("file:{}", skill_md.display());
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "bump skill".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "# v1\n\noriginal".into(),
            source: Some(source.clone()),
            importance: 4.0,
            source_path: Some(source),
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    let initial_updated_at: String = conn
        .query_row(
            "SELECT updated_at FROM mem_items WHERE id = ?1",
            params![&id],
            |r| r.get(0),
        )
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(10));

    let new_body = "# v2\n\nedited content longer to change size";
    std::fs::write(&skill_md, new_body).unwrap();

    let got = get(&conn, &id, &blobs).unwrap().unwrap();
    assert_eq!(got.body, new_body, "body must be re-read from disk");
    assert!(
        got.updated_at > initial_updated_at,
        "updated_at must bump when source file content changes (initial={}, after_get={})",
        initial_updated_at,
        got.updated_at,
    );
}

#[test]
fn get_skips_disk_check_when_recently_accessed() {
    let (conn, blobs) = fresh();
    let dir = std::env::temp_dir().join(format!("memstore-debounce-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let skill_md = dir.join("SKILL.md");
    std::fs::write(&skill_md, "# v1\n\noriginal").unwrap();

    let source = format!("file:{}", skill_md.display());
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "debounce skill".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "# v1\n\noriginal".into(),
            source: Some(source.clone()),
            importance: 4.0,
            source_path: Some(source),
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    // First get: warms accessed_at and metadata fields.
    let _ = get(&conn, &id, &blobs).unwrap().unwrap();
    let after_first: String = conn
        .query_row(
            "SELECT updated_at FROM mem_items WHERE id = ?1",
            params![&id],
            |r| r.get(0),
        )
        .unwrap();

    // Mutate file on disk WITHIN the debounce window.
    std::fs::write(&skill_md, "# v2\n\nedited within debounce window").unwrap();

    // Immediate re-get — debounce skips the metadata check; updated_at stays.
    let got = get(&conn, &id, &blobs).unwrap().unwrap();
    assert_eq!(
        got.updated_at, after_first,
        "updated_at must NOT bump within the 10s debounce window — was {} → {}",
        after_first, got.updated_at,
    );
}
