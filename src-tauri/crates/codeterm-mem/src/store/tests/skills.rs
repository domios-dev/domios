use codeterm_mem_store::skill_folders::{normalize_file_source, source_key, sync_skill_folders};
use rusqlite::{params, Connection};

use super::*;

fn skill_item(title: &str, source: &str, tags: Vec<String>) -> NewMemItem {
    NewMemItem {
        kind: MemKind::Skill,
        scope: MemScope::Global,
        title: title.into(),
        summary: None,
        area: None,
        tags,
        body: format!("# {title}\nbody"),
        source: Some(source.into()),
        importance: 4.0,
        source_path: Some(source.into()),
        pinned: false,
    }
}

fn write_skill(root: &std::path::Path, slug: &str, title: &str) -> String {
    let dir = root.join(slug);
    std::fs::create_dir_all(&dir).unwrap();
    let md = dir.join("SKILL.md");
    std::fs::write(&md, format!("---\ntitle: {title}\n---\n\n# {title}\n")).unwrap();
    format!("file:{}", md.display())
}

fn skill_count(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM mem_items WHERE kind = 'skill'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn source_key_normalizes_existing_skill_paths() {
    let root = tempfile::tempdir().unwrap();
    let canonical = write_skill(root.path(), "alpha", "Alpha Skill");
    let alternate = format!(
        "file:{}",
        root.path()
            .join("alpha")
            .join("..")
            .join("alpha")
            .join("SKILL.md")
            .display()
    );

    assert_eq!(source_key(&canonical), source_key(&alternate));
}

#[test]
fn source_key_normalizes_verbatim_windows_paths() {
    let verbatim = r"file:\\?\C:\Users\User\.codeterm\skills\qa-w4\SKILL.md";
    let plain = r"file:C:\Users\USER\.codeterm\skills\qa-w4\SKILL.md";

    assert_eq!(source_key(verbatim), source_key(plain));
    assert_eq!(
        normalize_file_source(verbatim),
        "file:c:/users/user/.codeterm/skills/qa-w4/skill.md"
    );
}

#[test]
fn skill_write_normalizes_file_sources() {
    let (conn, blobs) = fresh();
    let source = r"file:\\?\C:\Users\User\.codeterm\skills\qa-w4\SKILL.md";
    let id = insert(
        &conn,
        &skill_item("QA W4", source, vec!["folder-skill".into()]),
        &blobs,
    )
    .unwrap();
    let stored: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT source, source_path FROM mem_items WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    let expected = "file:c:/users/user/.codeterm/skills/qa-w4/skill.md";
    assert_eq!(stored.0.as_deref(), Some(expected));
    assert_eq!(stored.1.as_deref(), Some(expected));
}

#[test]
fn librarian_scan_does_not_twin_new_scaffold_skill() {
    let (conn, blobs) = fresh();
    let root = tempfile::tempdir().unwrap();
    let canonical = write_skill(root.path(), "qa-w4", "QA W4");
    let source = if cfg!(windows) {
        let plain = codeterm_paths::strip_verbatim_prefix(canonical.strip_prefix("file:").unwrap());
        format!("file:\\\\?\\{plain}")
    } else {
        canonical.clone()
    };
    let id = insert(
        &conn,
        &skill_item(
            "QA W4",
            &source,
            vec!["folder-skill".into(), "scaffold".into()],
        ),
        &blobs,
    )
    .unwrap();

    let report = sync_skill_folders(&conn, root.path());

    assert!(report.registered.is_empty());
    assert!(report.merged.is_empty());
    assert_eq!(skill_count(&conn), 1);
    assert!(conn
        .query_row(
            "SELECT 1 FROM mem_items WHERE id = ?1",
            params![id],
            |_| Ok(())
        )
        .is_ok());
}

#[test]
fn skill_delete_tombstone_survives_sync() {
    let (conn, blobs) = fresh();
    let root = tempfile::tempdir().unwrap();
    write_skill(root.path(), "alpha", "Alpha Skill");

    let first = sync_skill_folders(&conn, root.path());
    assert_eq!(first.registered, vec!["skill_alpha_skill".to_string()]);

    assert!(delete(&conn, "skill_alpha_skill").unwrap());
    let after_delete = sync_skill_folders(&conn, root.path());
    assert!(after_delete.registered.is_empty());
    assert_eq!(after_delete.skipped_tombstoned.len(), 1);
    assert_eq!(skill_count(&conn), 0);

    let source = format!(
        "file:{}",
        root.path().join("alpha").join("SKILL.md").display()
    );
    insert(&conn, &skill_item("Alpha Skill", &source, vec![]), &blobs).unwrap();
    assert!(delete(&conn, "skill_alpha_skill").unwrap());
    insert(&conn, &skill_item("Alpha Skill", &source, vec![]), &blobs).unwrap();
    let after_reregister = sync_skill_folders(&conn, root.path());
    assert!(after_reregister.skipped_tombstoned.is_empty());
    assert!(after_reregister.registered.is_empty());
    assert_eq!(skill_count(&conn), 1);
}

#[test]
fn skill_folder_single_registration() {
    let (conn, blobs) = fresh();
    let root = tempfile::tempdir().unwrap();
    let canonical = write_skill(root.path(), "x", "Bad Name x");
    let unnormalized = format!(
        "file:{}",
        root.path()
            .join("Bad Name")
            .join("..")
            .join("x")
            .join("SKILL.md")
            .display()
    );

    insert(
        &conn,
        &skill_item("Bad Name x", &unnormalized, vec![]),
        &blobs,
    )
    .unwrap();
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, tags_json, body_inline, importance, \
         created_at, updated_at, source_path) VALUES ('skill_bad_name____x', 'u-dup', 'skill', \
         'global', 'Bad Name x', '[\"folder-skill\",\"auto-registered\"]', 'b', 4.0, \
         '2099-01-01T00:00:00Z', '2099-01-01T00:00:00Z', ?1)",
        params![canonical],
    )
    .unwrap();
    assert_eq!(skill_count(&conn), 2);

    let report = sync_skill_folders(&conn, root.path());
    assert_eq!(report.merged.len(), 1);
    assert_eq!(report.merged[0].removed, "skill_bad_name____x");
    assert_eq!(report.merged[0].kept, "skill_bad_name_x");
    assert_eq!(skill_count(&conn), 1);
    assert!(report.registered.is_empty(), "folder already has its item");

    let again = sync_skill_folders(&conn, root.path());
    assert!(again.merged.is_empty() && again.registered.is_empty());
    assert_eq!(skill_count(&conn), 1);
}

#[test]
fn librarian_migrates_legacy_duplicate_and_keeps_explicit_skill() {
    let (conn, blobs) = fresh();
    let root = tempfile::tempdir().unwrap();
    let canonical = write_skill(root.path(), "qa-w4", "QA W4");
    let explicit_id = insert(
        &conn,
        &skill_item(
            "QA W4",
            &canonical,
            vec!["folder-skill".into(), "scaffold".into()],
        ),
        &blobs,
    )
    .unwrap();
    let legacy = if cfg!(windows) {
        let plain = codeterm_paths::strip_verbatim_prefix(canonical.strip_prefix("file:").unwrap());
        format!("file:\\\\?\\{plain}")
    } else {
        format!(
            "file:{}",
            root.path()
                .join("qa-w4")
                .join("..")
                .join("qa-w4")
                .join("SKILL.md")
                .display()
        )
    };
    conn.execute(
        "UPDATE mem_items SET source = ?1, source_path = ?1 WHERE id = ?2",
        params![legacy, explicit_id],
    )
    .unwrap();
    let auto_id = "skill_qa_w4_auto";
    conn.execute(
        "INSERT INTO mem_items
           (id, uid, kind, scope, title, tags_json, body_inline, source, importance,
            created_at, updated_at, source_path)
         VALUES (?1, 'qa-w4-auto', 'skill', 'global', 'QA W4 copy',
            '[\"folder-skill\",\"auto-registered\"]', '# QA W4', ?2, 4.0,
            '2099-01-01T00:00:00Z', '2099-01-01T00:00:00Z', ?2)",
        params![auto_id, canonical],
    )
    .unwrap();

    let report = sync_skill_folders(&conn, root.path());

    assert!(report.registered.is_empty());
    assert_eq!(report.merged.len(), 1);
    assert_eq!(report.merged[0].removed, auto_id);
    assert_eq!(report.merged[0].kept, explicit_id);
    assert_eq!(skill_count(&conn), 1);
    let stored_path: String = conn
        .query_row(
            "SELECT source_path FROM mem_items WHERE id = ?1",
            params![explicit_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored_path, normalize_file_source(&canonical));
}
