use super::*;
use codeterm_mem::backend::engines::EngineStore;
use codeterm_mem::embedder::ActiveEmbedder;
use std::cell::Cell;

fn terminate_fence(fixture: &Fixture) {
    let pid: i32 = fixture
        .store()
        .fence
        .client
        .lock()
        .query_one("SELECT pg_backend_pid()", &[])
        .unwrap()
        .get(0);
    assert!(fixture
        .client()
        .query_one("SELECT pg_terminate_backend($1, 5000)", &[&pid])
        .unwrap()
        .get::<_, bool>(0));
}

fn persisted(fixture: &Fixture) -> Vec<(String, Vec<String>)> {
    let mut client = fixture.client();
    let schema = &fixture.options.schema;
    client.query("SELECT tablename FROM pg_tables WHERE schemaname=$1 ORDER BY tablename", &[schema])
        .unwrap().into_iter().map(|row| {
            let table: String = row.get(0);
            let rows = client.query(&format!("SELECT to_jsonb(t)::text FROM {schema}.{table} t ORDER BY to_jsonb(t)::text"), &[])
                .unwrap().into_iter().map(|row| row.get(0)).collect();
            (table, rows)
        }).collect()
}

fn refused<T: std::fmt::Debug>(result: Result<T, String>) {
    let error = result.unwrap_err();
    assert!(
        error.contains("schema_busy") || error.contains("fence_lost"),
        "{error}"
    );
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn lost_fence_refuses_engine_shadow_purge_and_reads_before_callbacks() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let ctx = WriteContext::system("fence-regression");
    let active = ActiveEmbedder::new(Arc::new(super::engines::Provider {
        model: "old",
        dimension: 3,
    }));
    let target = ActiveEmbedder::new(Arc::new(super::engines::Provider {
        model: "new",
        dimension: 4,
    }));
    store
        .save(&ctx, &contract::item("ready shadow"), false)
        .unwrap();
    store.init_vectors(&active.identity()).unwrap();
    let stamp = serde_json::to_string(&target.identity()).unwrap();
    let (units, _) = store.shadow_stale(&stamp).unwrap();
    for unit in &units {
        store
            .shadow_checkpoint(
                &ctx,
                &stamp,
                unit,
                Some(&vec![0; unit.texts.len() * target.identity().dimension * 4]),
                false,
                &|| true,
            )
            .unwrap();
    }
    let id = "conversation_flush_abcdef12_pre_compact";
    let mut item = contract::item("flush:abcdef12 pre-compact");
    item.kind = codeterm_mem::MemKind::Conversation;
    item.body = "retired flush body ".repeat(300);
    assert_eq!(
        store
            .transaction(|tx| crate::items::insert_in(tx, &ctx, &item, None))
            .unwrap(),
        id
    );
    store.delete(&ctx, id).unwrap();
    let before = persisted(&fixture);
    let original = active.identity();
    terminate_fence(&fixture);
    let mut lease =
        Some(MigrationLease::try_acquire(&fixture.url, fixture.options.clone()).unwrap());
    for _ in 0..2 {
        let called = Cell::new(false);
        let claim = || {
            called.set(true);
            true
        };
        refused(store.publish_shadow(&ctx, &target, &active, &stamp, &claim));
        refused(store.shadow_checkpoint(&ctx, &stamp, &units[0], None, false, &claim));
        for wait in [true, false] {
            refused(store.clear_shadow(&ctx, wait));
            refused(store.prepare_engine_switch(&ctx, &target.identity(), 0, wait));
            refused(store.engine_status(wait));
        }
        refused(store.engine_plan(0));
        refused(store.purge_precompact(&ctx, id, true, true, true));
        refused(store.vacuum_precompact());
        refused(store.nontransactional_write(|_| {
            called.set(true);
            Ok(())
        }));
        refused(store.transaction(|_| {
            called.set(true);
            Ok(())
        }));
        refused(store.with_export_snapshot(&mut |_| {
            called.set(true);
            Ok(())
        }));
        assert!(store.sync_status(Duration::from_millis(100)).is_none());
        store.adopt_published(&ctx, &active, target.provider());
        assert!(!called.get());
        assert_eq!(active.identity(), original);
        assert_eq!(persisted(&fixture), before);
        drop(lease.take());
    }
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn vacuum_session_lease_survives_retained_fence_loss_on_executing_connection() {
    let fixture = Fixture::new();
    fixture.store().vacuum_precompact().unwrap();
    fixture
        .store()
        .nontransactional_write(|conn| {
            terminate_fence(&fixture);
            assert!(matches!(
                MigrationLease::try_acquire(&fixture.url, fixture.options.clone()),
                Err(PostgresOpenError::SchemaBusy)
            ));
            conn.batch_execute("VACUUM (ANALYZE) mem_fts, mem_item_history, mem_blobs")
                .map_err(db_error)
        })
        .unwrap();
    let _lease = MigrationLease::try_acquire(&fixture.url, fixture.options.clone()).unwrap();
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn snapshot_transaction_lease_survives_retained_fence_loss_and_refuses_result() {
    let fixture = Fixture::new();
    let result = fixture.store().with_export_snapshot(&mut |snapshot| {
        terminate_fence(&fixture);
        assert!(matches!(
            MigrationLease::try_acquire(&fixture.url, fixture.options.clone()),
            Err(PostgresOpenError::SchemaBusy)
        ));
        snapshot.table_counts()?;
        Ok(())
    });
    assert!(result.unwrap_err().contains("fence_lost"));
    let _lease = MigrationLease::try_acquire(&fixture.url, fixture.options.clone()).unwrap();
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn migration_claim_connection_supports_engine_shadow_snapshot_and_vacuum() {
    let mut fixture = Fixture::new();
    fixture.store.take();
    let lease = MigrationLease::try_acquire(&fixture.url, fixture.options.clone()).unwrap();
    let opened = lease.open(false).unwrap();
    let store = opened
        .fixture_backend()
        .downcast_ref::<PostgresMemStore>()
        .unwrap();
    let ctx = WriteContext::system("exclusive-engines");
    let active = ActiveEmbedder::new(Arc::new(super::engines::Provider {
        model: "old",
        dimension: 3,
    }));
    let target = ActiveEmbedder::new(Arc::new(super::engines::Provider {
        model: "new",
        dimension: 4,
    }));
    contract::engine_shadow_contract(store, &active, &target);
    for wait in [true, false] {
        store.clear_shadow(&ctx, wait).unwrap();
        store
            .prepare_engine_switch(&ctx, &target.identity(), 0, wait)
            .unwrap();
        store.engine_status(wait).unwrap();
    }
    assert!(store.sync_status(Duration::from_millis(100)).is_some());
    store
        .with_export_snapshot(&mut |snapshot| {
            snapshot.table_counts()?;
            Ok(())
        })
        .unwrap();
    store.vacuum_precompact().unwrap();
    drop(opened);
    drop(lease);
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn engine_try_operation_lock_refuses_competing_session() {
    let fixture = Fixture::new();
    let mut admin = fixture.client();
    let mut tx = admin.transaction().unwrap();
    tx.query_one(
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        &[&fixture.options.schema],
    )
    .unwrap();
    let ctx = WriteContext::system("operation-lock");
    assert!(fixture
        .store()
        .clear_shadow(&ctx, false)
        .unwrap_err()
        .contains("memory is busy"));
    assert!(fixture
        .store()
        .prepare_engine_switch(
            &ctx,
            &codeterm_mem::embedder::EmbedderIdentity {
                provider: "fixture".into(),
                model: "new".into(),
                dimension: 3,
            },
            0,
            false
        )
        .unwrap_err()
        .contains("memory is busy"));
    tx.rollback().unwrap();
    fixture.store().clear_shadow(&ctx, true).unwrap();
}
