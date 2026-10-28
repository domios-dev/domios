use super::{contract, Fixture};
use codeterm_mem::backend::{BlueprintStore, MemStore, WriteContext};

macro_rules! blueprint_case {
    ($name:ident) => {
        #[test]
        #[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL; nextest mem-postgres runs this case"]
        fn $name() {
            let fixture=Fixture::new();
            contract::blueprint::$name(fixture.store());
        }
    };
}

blueprint_case!(qualified_save_merge_archive_reuse);
blueprint_case!(scoped_save_eviction_and_rollback);
blueprint_case!(call_scope_hash_and_immutable_revisions);
blueprint_case!(dream_merge_and_archived_keeper);
blueprint_case!(enrichment_index_vectors_with_stamp);
blueprint_case!(curator_skill_metadata_merge);
blueprint_case!(source_filtered_solution_candidates);

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn blueprint_export_import_round_trip() {
    let source = Fixture::new();
    let target = Fixture::new();
    contract::blueprint::blueprint_export_import_round_trip(source.store(), target.store());
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn blueprint_sqlite_postgres_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let source = super::SqliteMemStore::open_in_memory(directory.path()).unwrap();
    let target = Fixture::new();
    contract::blueprint::blueprint_export_import_round_trip(&source, target.store());
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn blueprint_validation_archives_only_the_selected_scope() {
    let fixture = Fixture::new();
    let store = fixture.store();
    contract::blueprint::scoped_save_eviction_and_rollback(store);
    let ctx = WriteContext::system("validation-test");
    let id = store
        .blueprint_entries(&ctx, "contract-source", true, Some("team"))
        .unwrap()[0]
        .id
        .clone();
    MemStore::update(
        store,
        &ctx,
        &id,
        &codeterm_mem::store::MemUpdate {
            body: Some("invalid blueprint".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let before = super::export_all(store);
    assert!(store.blueprint_readonly("contract-source").is_err());
    assert_eq!(super::export_all(store), before);
    assert_eq!(
        store
            .blueprint_entries(&ctx, "contract-source", true, Some("other"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(super::export_all(store), before);
    assert!(store
        .blueprint_entries(&ctx, "contract-source", true, Some("team"))
        .unwrap()
        .is_empty());
    assert_eq!(MemStore::get(store, &id).unwrap().unwrap().kind, "archived");
    let history = MemStore::get_item_history(store, &id).unwrap();
    let last = history.last().unwrap();
    assert_eq!(last.author, ctx.author);
    assert_eq!(last.changed_by, "system:blueprint-library");
    assert_eq!(
        last.reason.as_deref(),
        Some("Blueprint recipe fails current validation")
    );
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn blueprint_revision_corruption_is_rejected() {
    let fixture = Fixture::new();
    let store = fixture.store();
    contract::blueprint::call_scope_hash_and_immutable_revisions(store);
    let (id,hash)=store.transaction(|tx| {
        let row=tx.query_one("SELECT item_id,hash FROM blueprint_call_revisions", &[]).map_err(crate::db_error)?;
        let id: String=row.get(0);
        let hash: String=row.get(1);
        tx.execute("UPDATE blueprint_call_revisions SET document=jsonb_set(document::jsonb,'{name}','\"tampered\"'::jsonb)::text", &[]).map_err(crate::db_error)?;
        Ok((id,hash))
    }).unwrap();
    let pin = codeterm_blueprint_types::blueprint_document::calls::CallPin {
        id: format!("mem:{id}"),
        hash,
    };
    assert_eq!(
        store
            .blueprint_call_revision("contract-source", "team", &pin)
            .unwrap_err(),
        "Pinned revision bytes are corrupted"
    );
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn blueprint_show_and_drift_are_readonly() {
    let fixture = Fixture::new();
    let store = fixture.store();
    contract::blueprint::scoped_save_eviction_and_rollback(store);
    let entry = store
        .blueprint_readonly("contract-source")
        .unwrap()
        .remove(0);
    let before = super::export_all(store);
    let shown = store
        .blueprint_solution_show("contract-source", &format!("mem:{}", entry.id))
        .unwrap();
    assert_eq!(shown.root["name"], entry.document.name);
    assert!(shown.calls.is_empty());
    assert!(store.blueprint_solution_show("foreign", &entry.id).is_err());
    assert!(store.blueprint_solution_show("", &entry.id).is_err());
    assert!(store
        .blueprint_call_drift("contract-source", "team", entry.document)
        .unwrap()
        .is_empty());
    assert_eq!(super::export_all(store), before);
}
