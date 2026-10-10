use super::*;
use crate::store::{MemKind, MemScope, NewMemItem};

const ID: &str = "conversation_flush_0123abcd_pre_compact";
fn fixture() -> (
    tempfile::TempDir,
    std::sync::Arc<MemManager>,
    String,
    String,
) {
    let dir = tempfile::tempdir().unwrap();
    let mem = MemManager::open_offline(dir.path()).unwrap();
    let body = "raw-fixture-only\n".repeat(9000);
    let name = blob_name(body.as_bytes());
    let id = mem
        .save(item("flush:0123abcd pre-compact", body.clone()))
        .unwrap();
    assert_eq!(id, ID);
    assert!(mem.blobs_dir.join(&name).exists());
    (dir, mem, body, name)
}
fn item(title: &str, body: String) -> NewMemItem {
    NewMemItem {
        kind: MemKind::Conversation,
        scope: MemScope::Global,
        title: title.into(),
        summary: Some("PTY scrollback flush from 0123abcd".into()),
        area: None,
        tags: vec!["flush".into(), "conversation".into(), "tab:0123abcd".into()],
        body,
        source: Some("tab:0123abcd".into()),
        importance: 1.0,
        source_path: None,
        pinned: false,
    }
}
fn history_count(mem: &MemManager) -> usize {
    mem.with_conn(|c| {
        c.query_row(
            "SELECT count(*) FROM mem_item_history WHERE item_id=?1",
            [ID],
            |r| r.get(0),
        )
        .unwrap()
    })
}

#[test]
fn precompact_purge_removes_retained_history_and_blob_after_ordinary_delete() {
    let (_dir, mem, _, name) = fixture();
    assert!(mem.delete(ID).unwrap());
    assert!(history_count(&mem) >= 2);
    assert!(mem.blobs_dir.join(&name).exists());
    let before = history_count(&mem);
    let preview = mem.purge_precompact(ID, false, false, false).unwrap();
    assert_eq!(preview.history_rows, before);
    assert!(!preview.logical_purge_complete);
    assert_eq!(history_count(&mem), before);
    let result = mem.purge_precompact(ID, true, true, true).unwrap();
    assert_eq!(result.history_rows, before);
    assert_eq!(result.history_rows_removed, before);
    assert!(result.history_body_bytes_removed > 0);
    assert!(result.logical_purge_complete && result.blob_cleanup_complete);
    assert_eq!(result.blobs_removed, 1);
    assert_eq!(result.vacuum, "completed");
    assert!(result.wal_checkpoint.starts_with("completed:"));
    assert!(!result.secure_erasure);
    assert_eq!(history_count(&mem), 0);
    assert!(!mem.blobs_dir.join(name).exists());
    assert!(result.errors.is_empty());
    let again = mem.purge_precompact(ID, true, true, false).unwrap();
    assert_eq!(again.blobs_removed, 0);
    assert_eq!(again.history_rows_removed, 0);
    assert_eq!(again.history_body_bytes_removed, 0);
    assert!(again.logical_purge_complete && again.blob_cleanup_complete);
}

#[test]
fn precompact_purge_requires_prior_receipt_and_deleted_exact_id() {
    let (_dir, mem, _, name) = fixture();
    assert!(mem
        .purge_precompact(ID, true, true, false)
        .unwrap_err()
        .contains("still live"));
    mem.delete(ID).unwrap();
    for id in [
        "fact_0123abcd",
        "conversation_flush_0123abcd_pre_compact_extra",
        "conversation_flush_0123abcX_pre_compact",
    ] {
        assert!(mem.purge_precompact(id, true, true, false).is_err());
    }
    assert!(mem.purge_precompact(ID, true, false, false).is_err());
    assert!(mem.purge_precompact(ID, false, false, true).is_err());
    assert!(mem
        .purge_precompact("conversation_flush_12345678_pre_compact", true, true, false)
        .is_err());
    assert!(history_count(&mem) >= 2);
    assert!(mem.blobs_dir.join(name).exists());
}

#[test]
fn precompact_purge_refuses_changed_kind_title_or_merge_without_mutation() {
    for (column, value) in [
        ("new_kind", "fact"),
        ("old_title", "curated knowledge"),
        ("change_type", "merge"),
    ] {
        let (_dir, mem, _, name) = fixture();
        mem.delete(ID).unwrap();
        mem.with_conn(|c| {
            c.execute(
                &format!("UPDATE mem_item_history SET {column}=?1 WHERE item_id=?2"),
                params![value, ID],
            )
            .unwrap()
        });
        let before = history_count(&mem);
        assert!(mem.purge_precompact(ID, true, true, false).is_err());
        assert_eq!(history_count(&mem), before);
        assert!(mem.blobs_dir.join(name).exists());
    }
}

#[test]
fn precompact_purge_preserves_live_and_historical_shared_blobs() {
    for delete_shared in [false, true] {
        let (_dir, mem, body, name) = fixture();
        let mut curated = item("kept knowledge", body);
        curated.kind = MemKind::Fact;
        curated.tags = vec!["curated".into()];
        curated.source = Some("user".into());
        let other = mem.save(curated).unwrap();
        if delete_shared {
            mem.delete(&other).unwrap();
        }
        mem.delete(ID).unwrap();
        let count: usize = mem.with_conn(|c| {
            c.query_row(
                "SELECT count(*) FROM mem_item_history WHERE item_id=?1",
                [&other],
                |r| r.get(0),
            )
            .unwrap()
        });
        let result = mem.purge_precompact(ID, true, true, false).unwrap();
        assert_eq!(result.shared_blobs_retained, 1);
        assert_eq!(result.blobs_removed, 0);
        assert!(mem.blobs_dir.join(name).exists());
        assert_eq!(mem.get(&other).unwrap().is_some(), !delete_shared);
        assert_eq!(
            mem.with_conn(|c| c
                .query_row(
                    "SELECT count(*) FROM mem_item_history WHERE item_id=?1",
                    [&other],
                    |r| r.get::<_, usize>(0)
                )
                .unwrap()),
            count
        );
    }
}

#[test]
fn precompact_purge_retains_retry_metadata_after_blob_failure() {
    let (_dir, mem, body, name) = fixture();
    mem.delete(ID).unwrap();
    std::fs::write(mem.blobs_dir.join(&name), "changed fixture").unwrap();
    let result = mem.purge_precompact(ID, true, true, false).unwrap();
    assert!(result.logical_purge_complete);
    assert!(!result.blob_cleanup_complete);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(history_count(&mem), 0);
    std::fs::write(mem.blobs_dir.join(&name), body).unwrap();
    let result = mem.purge_precompact(ID, true, true, false).unwrap();
    assert!(result.blob_cleanup_complete);
    assert_eq!(result.blobs_removed, 1);
}

#[test]
fn precompact_purge_rolls_back_history_if_database_deletion_fails() {
    let (_dir, mem, _, name) = fixture();
    mem.delete(ID).unwrap();
    mem.with_conn(|c| c.execute_batch("CREATE TRIGGER refuse_receipt BEFORE INSERT ON mem_precompact_purges BEGIN SELECT RAISE(ABORT,'fixture refusal'); END;").unwrap());
    let before = history_count(&mem);
    assert!(mem.purge_precompact(ID, true, true, false).is_err());
    assert_eq!(history_count(&mem), before);
    assert!(mem.blobs_dir.join(name).exists());
}

#[test]
fn precompact_purge_only_removes_exact_vector_chunk_ids_and_preserves_graph() {
    let (_dir, mem, _, _) = fixture();
    mem.delete(ID).unwrap();
    mem.with_conn(|c| {
        c.execute_batch("CREATE TABLE IF NOT EXISTS mem_vectors(mem_id TEXT PRIMARY KEY, embedding BLOB);")
            .unwrap();
        for id in [
            ID.to_string(),
            format!("{ID}_chunk_0"),
            format!("{ID}_chunk_42"),
            format!("{ID}_chunk_notes"),
            format!("{ID}x_chunk_0"),
        ] {
            c.execute("INSERT INTO mem_vectors VALUES(?1,zeroblob(1536))", [id])
                .unwrap();
        }
        c.execute(
            "INSERT INTO entities(id,entity_type,scope,properties_json,created_at,updated_at) VALUES('kept','fact','global','{}','now','now')",
            [],
        )
        .unwrap();
    });
    let result = mem.purge_precompact(ID, true, true, false).unwrap();
    assert_eq!(result.vectors_removed, 3);
    mem.with_conn(|c| {
        assert_eq!(
            c.query_row("SELECT count(*) FROM mem_vectors", [], |r| r
                .get::<_, usize>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            c.query_row("SELECT count(*) FROM entities WHERE id='kept'", [], |r| {
                r.get::<_, usize>(0)
            })
            .unwrap(),
            1
        );
    });
}

#[test]
fn precompact_purge_refuses_recreated_history_and_overlarge_bodies() {
    let (_dir, mem, _, _) = fixture();
    mem.delete(ID).unwrap();
    mem.purge_precompact(ID, true, true, false).unwrap();
    mem.with_conn(|c| c.execute("INSERT INTO mem_item_history(item_id,changed_at,changed_by,change_type,new_title,new_kind) VALUES(?1,'now','fixture','create','flush:0123abcd pre-compact','conversation')",[ID]).unwrap());
    assert!(mem
        .purge_precompact(ID, true, true, false)
        .unwrap_err()
        .contains("recreated"));
    assert_eq!(history_count(&mem), 1);
    mem.with_conn(|c| {
        c.execute(
            "UPDATE mem_item_history SET new_body=zeroblob(?1) WHERE item_id=?2",
            params![MAX_BODY_BYTES + 1, ID],
        )
        .unwrap()
    });
    assert!(mem
        .purge_precompact(ID, true, true, false)
        .unwrap_err()
        .contains("bounded body"));
}

#[cfg(unix)]
#[test]
fn precompact_purge_refuses_symlink_blob_and_preserves_target() {
    let (dir, mem, body, name) = fixture();
    mem.delete(ID).unwrap();
    let outside = dir.path().join("kept.txt");
    std::fs::write(&outside, body).unwrap();
    std::fs::remove_file(mem.blobs_dir.join(&name)).unwrap();
    std::os::unix::fs::symlink(&outside, mem.blobs_dir.join(&name)).unwrap();
    let result = mem.purge_precompact(ID, true, true, false).unwrap();
    assert!(!result.blob_cleanup_complete);
    assert!(outside.exists());
    assert!(std::fs::symlink_metadata(mem.blobs_dir.join(name))
        .unwrap()
        .file_type()
        .is_symlink());
}
