use super::*;
use crate::backend::{migrate, SqliteMemStore, StoreTable};
use std::sync::Arc;

fn fixture(dir: &std::path::Path) -> Arc<SqliteMemStore> {
    Arc::new(SqliteMemStore::open_in_memory(dir).unwrap())
}

#[test]
fn transfer_sqlite_round_trip_all_21_tables_items_history_and_authors() {
    let dir = tempfile::tempdir().unwrap();
    let source = fixture(&dir.path().join("source"));
    let target = fixture(&dir.path().join("target"));
    let staging = fixture(&dir.path().join("staging"));
    export_import_round_trip(source.as_ref(), staging.as_ref());
    let ctx = WriteContext::new(
        MemAuthor::neighbor("neighbor".into(), "friendship".into()),
        "transfer-update",
    );
    let mut payload = item("transfer history");
    let id = source.save(&ctx, &payload, false).unwrap().id;
    payload.body = "updated transfer body".into();
    source
        .save(
            &WriteContext::new(MemAuthor::owner(Some("owner".into())), "owner-update"),
            &payload,
            true,
        )
        .unwrap();
    source.conn.lock().execute_batch(&format!(
        "INSERT INTO solution_metadata(item_id,metadata_json) VALUES ('{id}','{{}}');
         INSERT INTO blueprint_call_scopes(item_id,area,team) VALUES ('{id}','area','team');
         INSERT INTO blueprint_call_revisions(item_id,hash,area,team,document) VALUES ('{id}','hash','area','team','{{}}');
         INSERT INTO blueprint_index(item_id,document_hash,canon,gists,signature_json) VALUES ('{id}','hash','canon','gist','{{}}');
         INSERT INTO blueprint_vectors(item_id,slot,ordinal,text,embedding,provider,model,dimension) VALUES ('{id}','canon',0,'canon',X'0000803f0000000000000000','contract','migration',3);
         INSERT INTO blueprint_v1_cleanup_receipt(id,removed_count,completed_at) VALUES (1,7,'2026-01-01');
         INSERT INTO mem_source_tombstones(source_key,deleted_at) VALUES ('source','2026-01-01');
         INSERT INTO mem_precompact_purges(item_id,history_rows,history_body_bytes,purged_at) VALUES ('deleted',1,5,'2026-01-01');
         INSERT INTO mem_precompact_purge_blobs(item_id,path,body_bytes) VALUES ('deleted','blob',5);
         INSERT INTO mem_vector_pending(mem_id,reason,queued_at) VALUES ('{id}','pending','2026-01-01');
         INSERT INTO lesson_audit(lesson_uid,machine_id,action,ts) VALUES ('lesson','machine','promoted','2026-01-01');
         INSERT INTO graph_purge_log(purged_at,count,actor) VALUES ('2026-01-01',1,'owner');"
    )).unwrap();
    let source_dyn: Arc<dyn MemStore> = source.clone();
    let target_dyn: Arc<dyn MemStore> = target.clone();
    let before = export_all(source.as_ref());
    let counts = source.table_counts().unwrap();
    for table in StoreTable::ALL {
        assert!(counts[&table] > 0, "missing populated fixture {table:?}");
    }
    let copied = migrate::transfer(&source_dyn, &target_dyn, false).unwrap();
    assert_eq!(copied.len(), 21);
    assert_eq!(copied, counts);
    assert_eq!(export_all(target.as_ref()), before);
    assert_eq!(export_all(source.as_ref()), before);
    assert_eq!(
        target.get(&id).unwrap().unwrap().author,
        source.get(&id).unwrap().unwrap().author
    );
    assert_eq!(
        serde_json::to_value(target.get_item_history(&id).unwrap()).unwrap(),
        serde_json::to_value(source.get_item_history(&id).unwrap()).unwrap()
    );
}

#[test]
fn transfer_refuses_nonempty_target_and_same_handle_without_writes() {
    let dir = tempfile::tempdir().unwrap();
    let source: Arc<dyn MemStore> = fixture(&dir.path().join("source"));
    let target: Arc<dyn MemStore> = fixture(&dir.path().join("target"));
    source
        .save(&WriteContext::system("source"), &item("source"), false)
        .unwrap();
    target
        .save(&WriteContext::system("target"), &item("target"), false)
        .unwrap();
    let before = export_all(target.as_ref());
    let source_before = export_all(source.as_ref());
    assert!(migrate::transfer(&source, &target, false)
        .unwrap_err()
        .contains("target_nonempty"));
    assert_eq!(export_all(target.as_ref()), before);
    assert_eq!(export_all(source.as_ref()), source_before);
    assert!(migrate::transfer(&source, &source, true)
        .unwrap_err()
        .contains("same_store"));
}

#[test]
fn transfer_verification_rejects_missing_and_mismatched_tables() {
    let counts: crate::backend::TableCounts = StoreTable::ALL
        .into_iter()
        .map(|table| (table, 0))
        .collect();
    let mut other = counts.clone();
    other.remove(&StoreTable::History);
    assert!(migrate::verify_counts(&counts, &other).is_err());
    other.insert(StoreTable::History, 1);
    assert!(migrate::verify_counts(&counts, &other)
        .unwrap_err()
        .contains("History"));
}

#[test]
fn transfer_read_only_source_refuses_writes_and_schema_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("knowledge.db");
    let blobs = dir.path().join("blobs");
    {
        let source = SqliteMemStore::open(&db, &blobs).unwrap();
        source
            .save(&WriteContext::system("source"), &item("source"), false)
            .unwrap();
    }
    let bytes = std::fs::read(&db).unwrap();
    {
        let source = SqliteMemStore::open_read_only(&db, &blobs).unwrap();
        assert!(source
            .save(
                &WriteContext::system("forbidden"),
                &item("forbidden"),
                false
            )
            .is_err());
        assert_eq!(source.table_counts().unwrap()[&StoreTable::Items], 1);
    }
    assert_eq!(std::fs::read(&db).unwrap(), bytes);
    let obsolete = dir.path().join("obsolete.db");
    let conn = rusqlite::Connection::open(&obsolete).unwrap();
    conn.execute_batch("CREATE TABLE mem_schema_version(version INTEGER); INSERT INTO mem_schema_version VALUES(0)").unwrap();
    drop(conn);
    assert!(matches!(
        SqliteMemStore::open_read_only(&obsolete, &blobs),
        Err(crate::backend::sqlite::ReadOnlyOpenError::SourceSchemaMismatch)
    ));
}

#[test]
fn transfer_streams_multiple_pages_within_a_table() {
    let dir = tempfile::tempdir().unwrap();
    let source: Arc<dyn MemStore> = fixture(&dir.path().join("source"));
    let target: Arc<dyn MemStore> = fixture(&dir.path().join("target"));
    for index in 0..257 {
        source
            .save(
                &WriteContext::system("source"),
                &item(&format!("paged item {index}")),
                false,
            )
            .unwrap();
    }
    let before = export_all(source.as_ref());
    let copied = migrate::transfer(&source, &target, false).unwrap();
    assert_eq!(copied[&StoreTable::Items], 257);
    assert_eq!(export_all(source.as_ref()), before);
    assert_eq!(export_all(target.as_ref()), before);
}

#[test]
fn transfer_same_stamp_import_preserves_existing_target_vectors() {
    let dir = tempfile::tempdir().unwrap();
    let source = fixture(&dir.path().join("source"));
    let target = fixture(&dir.path().join("target"));
    let ctx = WriteContext::system("fixture");
    let identity = EmbedderIdentity {
        provider: "fixture".into(),
        model: "same".into(),
        dimension: 3,
    };
    source.init_vectors(&ctx, &identity).unwrap();
    target.init_vectors(&ctx, &identity).unwrap();
    let id = target.save(&ctx, &item("target vector"), false).unwrap().id;
    target
        .insert_vector(
            &ctx,
            &id,
            0,
            &StampedVector {
                identity,
                vector: vec![1.0, 0.0, 0.0],
            },
        )
        .unwrap();
    let before = export_all(target.as_ref());
    let page = export_all(source.as_ref())
        .into_iter()
        .find(|page| page.table == StoreTable::Embedder)
        .unwrap();
    target.import_page(&ctx, &page).unwrap();
    assert_eq!(export_all(target.as_ref()), before);
}
