use codeterm_mem_store::skill_folders::{source_key, sync_skill_folders};
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
