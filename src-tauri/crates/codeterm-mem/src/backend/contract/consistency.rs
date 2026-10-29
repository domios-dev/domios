use super::*;
use crate::backend::{migrate, SqliteMemStore, StoreTable};
use std::sync::Arc;

#[test]
fn sqlite_snapshot_keeps_committed_between_page_changes_invisible() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let blobs = dir.path().join("source-blobs");
    let source = Arc::new(SqliteMemStore::open(&db, &blobs).unwrap());
    let ctx = WriteContext::system("snapshot-fixture");
    for index in 0..257 {
        source
            .save(&ctx, &item(&format!("snapshot {index:04}")), false)
            .unwrap();
    }
    let before = migrate::content_digests(source.as_ref()).unwrap();
    let source_counts = source.table_counts().unwrap();
    let target =
        Arc::new(SqliteMemStore::open_in_memory(&dir.path().join("target-blobs")).unwrap());
    let source_dyn: Arc<dyn MemStore> = source.clone();
    let target_dyn: Arc<dyn MemStore> = target.clone();
    let mut writer = rusqlite::Connection::open(&db).unwrap();
    let mut changed = false;
    let proof = migrate::transfer_with_hook(&source_dyn, &target_dyn, false, &mut |page| {
        if !changed && page.table == StoreTable::Items {
            let tx = writer.transaction().unwrap();
            assert_eq!(
                tx.execute(
                    "UPDATE mem_items SET summary='committed-after-first-page'",
                    []
                )
                .unwrap(),
                257
            );
            tx.commit().unwrap();
            changed = true;
        }
        Ok(())
    })
    .unwrap();
    assert!(changed);
    assert_eq!(proof.source_counts, source_counts);
    assert_eq!(proof.source_digests, before);
    assert_eq!(proof.target_digests, before);
    assert_ne!(migrate::content_digests(source.as_ref()).unwrap(), before);
}

#[test]
fn content_verification_refuses_tampered_target_row_and_blob() {
    for tamper_blob in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let source = Arc::new(SqliteMemStore::open_in_memory(&dir.path().join("source")).unwrap());
        let target = Arc::new(SqliteMemStore::open_in_memory(&dir.path().join("target")).unwrap());
        let mut payload = item("content proof");
        payload.body = "large body blob ".repeat(1000);
        source
            .save(&WriteContext::system("content-fixture"), &payload, false)
            .unwrap();
        let source_dyn: Arc<dyn MemStore> = source;
        let target_dyn: Arc<dyn MemStore> = target.clone();
        let result = migrate::transfer_with_hook(&source_dyn, &target_dyn, false, &mut |page| {
            if page.table == StoreTable::Items {
                if tamper_blob {
                    let name = page.rows[0].blobs.keys().next().unwrap();
                    std::fs::write(target.blobs_dir.join(name), b"tampered").unwrap();
                } else {
                    target
                        .conn
                        .lock()
                        .execute("UPDATE mem_items SET summary='tampered'", [])
                        .unwrap();
                }
            }
            Ok(())
        });
        assert!(result.unwrap_err().contains("digest_mismatch"));
        assert_eq!(target.table_counts().unwrap()[&StoreTable::Items], 1);
    }
}

#[test]
fn into_nonempty_verifies_union_and_refuses_collisions() {
    let dir = tempfile::tempdir().unwrap();
    let source = Arc::new(SqliteMemStore::open_in_memory(&dir.path().join("source")).unwrap());
    let target = Arc::new(SqliteMemStore::open_in_memory(&dir.path().join("target")).unwrap());
    let ctx = WriteContext::system("union-fixture");
    source
        .graph_create_entity(
            &ctx,
            &crate::Entity {
                id: "source-entity".into(),
                entity_type: "Concept".into(),
                scope: "global".into(),
                properties: serde_json::json!({"source":true}),
            },
        )
        .unwrap();
    target
        .graph_create_entity(
            &ctx,
            &crate::Entity {
                id: "target-entity".into(),
                entity_type: "Concept".into(),
                scope: "global".into(),
                properties: serde_json::json!({"target":true}),
            },
        )
        .unwrap();
    let source_dyn: Arc<dyn MemStore> = source;
    let target_dyn: Arc<dyn MemStore> = target.clone();
    let proof = migrate::transfer_verified(&source_dyn, &target_dyn, true).unwrap();
    assert_eq!(proof.target_counts[&StoreTable::Entities], 2);
    assert_ne!(
        proof.source_digests[&StoreTable::Entities],
        proof.target_digests[&StoreTable::Entities]
    );
    assert_eq!(
        proof.target_digests,
        migrate::content_digests(target.as_ref()).unwrap()
    );
    assert!(migrate::transfer(&source_dyn, &target_dyn, true).is_err());
}
