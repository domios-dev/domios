use super::*;
use codeterm_mem::backend::engines::{EngineStore, SourceRefresh};
use codeterm_mem::embedder::{provider::*, ActiveEmbedder};

pub(super) struct Provider {
    pub(super) model: &'static str,
    pub(super) dimension: usize,
}
impl EmbeddingProvider for Provider {
    fn id(&self) -> &str {
        "contract"
    }
    fn model(&self) -> &str {
        self.model
    }
    fn dimension(&self) -> usize {
        self.dimension
    }
    fn max_tokens(&self) -> usize {
        8192
    }
    fn modalities(&self) -> &[Modality] {
        &[Modality::Text]
    }
    fn remote(&self) -> bool {
        false
    }
    fn embed_passage(&self, _: &Content) -> Result<Vec<f32>, EmbedError> {
        panic!("storage must never run inference")
    }
    fn embed_query(&self, _: &Content) -> Result<Vec<f32>, EmbedError> {
        panic!("storage must never run inference")
    }
}
#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn pending_contract() {
    let fixture = Fixture::new();
    contract::engine_pending_contract(fixture.store());
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn production_open_uses_migrated_schema() {
    let fixture = Fixture::new();
    let store = open(&fixture.url, fixture.options.clone()).unwrap();
    let ctx = WriteContext::system("production-open");
    let id = store
        .save(&ctx, &contract::item("production open"), false)
        .unwrap()
        .id;
    assert_eq!(
        fixture.store().get(&id).unwrap().unwrap().title,
        "production open"
    );
    contract::engine_pending_contract(store.as_ref());
    assert!(store.blueprint_indexed_items().unwrap().is_empty());
}
#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn auxiliary_round_trip_contract() {
    let source = Fixture::new();
    let target = Fixture::new();
    contract::engine_auxiliary_round_trip(source.store(), target.store());
}
#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn shadow_publication_contract() {
    let fixture = Fixture::new();
    let active = ActiveEmbedder::new(Arc::new(Provider {
        model: "old",
        dimension: 3,
    }));
    let target = ActiveEmbedder::new(Arc::new(Provider {
        model: "new",
        dimension: 4,
    }));
    contract::engine_shadow_contract(fixture.store(), &active, &target);
}
#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn source_hydration_matches_sqlite_and_refresh_is_compare_and_swap() {
    let fixture = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let sqlite = SqliteMemStore::open_in_memory(&dir.path().join("blobs")).unwrap();
    let path = dir.path().join("source.txt");
    std::fs::write(&path, "first body").unwrap();
    let ctx = WriteContext::system("source-contract");
    let mut item = contract::item("tracked source");
    item.source = Some(format!("file:{}", path.display()));
    item.source_path = item.source.clone();
    let id = fixture.store().save(&ctx, &item, false).unwrap().id;
    sqlite.save(&ctx, &item, false).unwrap();
    std::fs::write(&path, "  replacement body with different size\n").unwrap();
    let pg = fixture.store().get(&id).unwrap().unwrap();
    let sq = sqlite.get(&id).unwrap().unwrap();
    assert_eq!(pg.body, "replacement body with different size");
    assert_eq!(pg.body, sq.body);
    let before = fixture.store().tracked_sources().unwrap().remove(0);
    assert_eq!(
        before.source_size,
        Some(std::fs::metadata(&path).unwrap().len() as i64)
    );
    let refresh = SourceRefresh {
        body: Some("persisted sync body"),
        size: Some(99),
        mtime: Some("changed"),
        missing: false,
    };
    assert!(fixture
        .store()
        .refresh_source(&ctx, &before, &refresh)
        .unwrap());
    assert!(!fixture
        .store()
        .refresh_source(&ctx, &before, &refresh)
        .unwrap());
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        fixture.store().get(&id).unwrap().unwrap().body,
        "persisted sync body"
    );
}
#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn shadow_targets_do_not_touch_access_or_metadata_and_busy_does_not_wait() {
    let fixture = Fixture::new();
    let store = fixture.store();
    store
        .save(
            &WriteContext::system("target"),
            &contract::item("untouched"),
            false,
        )
        .unwrap();
    let before = export_all(store);
    assert_eq!(store.reembed_item_targets().unwrap().len(), 1);
    assert!(store.missing_item_vectors().is_empty());
    assert!(store.missing_entity_vectors().is_err());
    assert_eq!(before, export_all(store));
    store
        .init_vectors(&EmbedderIdentity {
            provider: "fixture".into(),
            model: "one".into(),
            dimension: 3,
        })
        .unwrap();
    assert_eq!(store.missing_item_vectors().len(), 1);
    let _guard = store.gate.lock();
    assert!(store.engine_status(false).unwrap_err().contains("busy"));
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn purge_receipt_is_repeatable_and_preserves_shared_blobs() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let ctx = WriteContext::system("purge-fixture");
    let id = "conversation_flush_abcdef12_pre_compact";
    let body = "shared body ".repeat(300);
    let mut item = contract::item("flush:abcdef12 pre-compact");
    item.kind = codeterm_mem::MemKind::Conversation;
    item.body = body.clone();
    assert_eq!(
        store
            .transaction(|tx| crate::items::insert_in(tx, &ctx, &item, None))
            .unwrap(),
        id
    );
    let mut keeper = contract::item("keeper");
    keeper.body = body;
    store.save(&ctx, &keeper, false).unwrap();
    assert!(store
        .purge_precompact(&ctx, id, false, false, false)
        .is_err());
    store.delete(&ctx, id).unwrap();
    let preview = store
        .purge_precompact(&ctx, id, false, false, false)
        .unwrap();
    assert!(!preview.logical_purge_complete);
    assert!(preview.history_body_bytes > 2048);
    assert!(store
        .purge_precompact(&ctx, id, true, false, false)
        .is_err());
    let applied = store.purge_precompact(&ctx, id, true, true, false).unwrap();
    assert!(applied.logical_purge_complete && applied.blob_cleanup_complete);
    assert_eq!(applied.shared_blobs_retained, 1);
    assert_eq!(applied.history_rows_removed, preview.history_rows);
    let repeated = store.purge_precompact(&ctx, id, true, true, false).unwrap();
    assert_eq!(repeated.history_rows_removed, 0);
    assert_eq!(repeated.shared_blobs_retained, 1);
    assert_eq!(
        store
            .transaction(|tx| Ok(tx
                .query_one("SELECT count(*) FROM mem_blobs", &[])
                .map_err(db_error)?
                .get::<_, i64>(0)))
            .unwrap(),
        1
    );
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn skill_folders_and_layout_read_models_persist() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("skills");
    std::fs::create_dir_all(root.join("fixture")).unwrap();
    std::fs::write(
        root.join("fixture/SKILL.md"),
        "# Fixture skill\n\nDo the fixture work.",
    )
    .unwrap();
    let ctx = WriteContext::system("skill-contract");
    let report = store.sync_skill_folders(&ctx, &root);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.registered.len(), 1);
    assert!(store.sync_skill_folders(&ctx, &root).registered.is_empty());
    let id = &report.registered[0];
    let cache =
        codeterm_mem::graph_layout::cache::LayoutCache::load(dir.path().join("layout.json"));
    let snapshot = store.layout_snapshot(&cache).unwrap();
    assert_eq!(snapshot.nodes.len(), 1);
    assert_eq!(snapshot.nodes[0].id, *id);
    assert_eq!(
        store.layout_labels(std::slice::from_ref(id)).unwrap()[0].1,
        "Fixture skill"
    );
    assert!(store
        .layout_edges(std::slice::from_ref(id))
        .unwrap()
        .is_empty());
    assert!(store
        .rem_snapshot(2)
        .unwrap()
        .contains("merged 2 duplicate graphs"));
}
