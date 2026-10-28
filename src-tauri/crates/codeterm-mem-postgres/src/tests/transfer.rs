use super::*;
use codeterm_mem::backend::{ExportPage, StoreRow, StoreValue};

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn postgres_preserves_every_transfer_table_from_sqlite() {
    let source_dir = tempfile::tempdir().unwrap();
    let source = SqliteMemStore::open_in_memory(source_dir.path()).unwrap();
    let intermediate = Fixture::new();
    contract::export_import_round_trip(&source, intermediate.store());
    let ctx = WriteContext::system("transfer-fixture");
    let text = |value: &str| StoreValue::Text(value.to_owned());
    let int = StoreValue::Integer;
    let id = "fact_contract_migration";
    let rows=[
        (StoreTable::SolutionMetadata,vec![("item_id",text(id)),("metadata_json",text("{\"evidence\":{},\"content_fingerprint\":null,\"blueprint\":null,\"curator\":null}"))]),
        (StoreTable::BlueprintCallScopes,vec![("item_id",text(id)),("area",text("transfer")),("team",text("transfer-team"))]),
        (StoreTable::BlueprintCallRevisions,vec![("item_id",text(id)),("hash",text("opaque-hash")),("area",text("transfer")),("team",text("transfer-team")),("document",text("opaque immutable transfer document"))]),
        (StoreTable::BlueprintIndex,vec![("item_id",text(id)),("document_hash",text("opaque-hash")),("canon",text("canon")),("gists",text("gist")),("signature_json",text("{}")),("enrichment_json",StoreValue::Null)]),
        (StoreTable::BlueprintVectors,vec![("item_id",text(id)),("slot",text("canon")),("ordinal",int(0)),("text",text("canon")),("embedding",StoreValue::Bytes(codeterm_mem::embedder::index::encode(&[1.0,0.0,0.0]))),("provider",text("contract")),("model",text("migration")),("dimension",int(3))]),
        (StoreTable::BlueprintCleanupReceipt,vec![("id",int(1)),("removed_count",int(7)),("completed_at",text("2026-01-01 00:00:00"))]),
        (StoreTable::SourceTombstones,vec![("source_key",text("file:/disposable/skill/SKILL.md")),("deleted_at",text("2026-01-01T00:00:00Z"))]),
        (StoreTable::PrecompactPurges,vec![("item_id",text(id)),("history_rows",int(2)),("history_body_bytes",int(64)),("purged_at",text("2026-01-01T00:00:00Z"))]),
        (StoreTable::PrecompactBlobs,vec![("item_id",text(id)),("path",text("sha256_disposable.txt")),("body_bytes",int(32))]),
        (StoreTable::VectorPending,vec![("mem_id",text("pending-transfer")),("reason",text("offline")),("queued_at",text("2026-01-01T00:00:00Z"))]),
        (StoreTable::LessonAudit,vec![("id",int(7)),("lesson_uid",text("lesson-uid")),("machine_id",text("machine")),("action",text("promoted")),("ts",text("2026-01-01T00:00:00Z")),("reason",text("transfer proof"))]),
        (StoreTable::GraphPurgeLog,vec![("id",int(9)),("purged_at",text("2026-01-01T00:00:00Z")),("count",int(2)),("scope",StoreValue::Null),("before",StoreValue::Null),("reason_filter",StoreValue::Null),("actor",text("transfer-proof")),("details_json",text("{\"preserved\":true}"))]),
    ];
    for (table, values) in rows {
        if source.table_counts().unwrap()[&table] > 0 {
            continue;
        }
        source
            .import_page(
                &ctx,
                &ExportPage {
                    table,
                    rows: vec![StoreRow {
                        values: values
                            .into_iter()
                            .map(|(key, value)| (key.to_owned(), value))
                            .collect(),
                        blobs: Default::default(),
                    }],
                    next: None,
                },
            )
            .unwrap();
    }
    let expected = source.table_counts().unwrap();
    for table in StoreTable::ALL {
        assert!(expected[&table] > 0, "{table:?} must have a transfer row");
    }
    let target = Fixture::new();
    let pages = export_all(&source);
    for page in &pages {
        MemStore::import_page(target.store(), &ctx, page).unwrap();
    }
    assert_eq!(target.store().table_counts().unwrap(), expected);
    assert_eq!(export_all(target.store()), pages);
    let restored_dir = tempfile::tempdir().unwrap();
    let restored = SqliteMemStore::open_in_memory(restored_dir.path()).unwrap();
    for page in export_all(target.store()) {
        restored.import_page(&ctx, &page).unwrap();
    }
    assert_eq!(restored.table_counts().unwrap(), expected);
    assert_eq!(export_all(&restored), pages);
}
