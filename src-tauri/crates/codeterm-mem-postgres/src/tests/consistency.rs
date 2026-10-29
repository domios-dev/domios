use super::*;
use codeterm_mem::backend::migrate;

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn postgres_repeatable_read_snapshot_hides_committed_between_page_changes() {
    let mut fixture = Fixture::new();
    let schema = fixture.options.schema.clone();
    fixture.client().batch_execute(&format!("INSERT INTO {schema}.entities(id,entity_type,scope,properties_json,created_at,updated_at) SELECT 'entity_' || lpad(n::text,4,'0'), 'Concept','global','{{\"before\":true}}','before','before' FROM generate_series(1,257) n")).unwrap();
    let before = migrate::content_digests(fixture.store()).unwrap();
    fixture.store.take();
    let lease = MigrationLease::try_acquire(&fixture.url, fixture.options.clone()).unwrap();
    let source = lease.open(true).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let target: Arc<dyn MemStore> = Arc::new(SqliteMemStore::open_in_memory(dir.path()).unwrap());
    let mut writer = fixture.client();
    let mut changed = false;
    let proof = migrate::transfer_with_hook(&source, &target, false, &mut |page| {
        if !changed && page.table == StoreTable::Entities {
            assert_eq!(page.rows.len(), 256);
            assert_eq!(
                writer
                    .execute(
                        &format!(
                            "UPDATE {schema}.entities SET properties_json='{{\"after\":true}}'"
                        ),
                        &[]
                    )
                    .unwrap(),
                257
            );
            changed = true;
        }
        Ok(())
    })
    .unwrap();
    assert!(changed);
    assert_eq!(proof.source_digests, before);
    assert_eq!(proof.target_digests, before);
    assert_ne!(migrate::content_digests(source.as_ref()).unwrap(), before);
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn schema_fence_refuses_migrate_on_either_side_and_store_during_migrate() {
    for postgres_source in [true, false] {
        let mut fixture = Fixture::new();
        assert!(matches!(
            MigrationLease::try_acquire(&fixture.url, fixture.options.clone()),
            Err(PostgresOpenError::SchemaBusy)
        ));
        fixture.store.take();
        let lease = MigrationLease::try_acquire(&fixture.url, fixture.options.clone()).unwrap();
        let postgres = lease.open(postgres_source).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let sqlite: Arc<dyn MemStore> =
            Arc::new(SqliteMemStore::open_in_memory(dir.path()).unwrap());
        let (source, target) = if postgres_source {
            (postgres, sqlite)
        } else {
            (sqlite, postgres)
        };
        let mut observed = false;
        migrate::transfer_with_hook(&source, &target, false, &mut |_| {
            assert!(matches!(
                open(&fixture.url, fixture.options.clone()),
                Err(PostgresOpenError::SchemaBusy)
            ));
            observed = true;
            Ok(())
        })
        .unwrap();
        assert!(observed);
        drop(source);
        drop(target);
        drop(lease);
        assert!(open(&fixture.url, fixture.options.clone()).is_ok());
    }
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn recycling_pool_preserves_retained_fence_and_lost_fence_refuses_writes() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let fence_pid: i32 = store
        .fence
        .client
        .lock()
        .query_one("SELECT pg_backend_pid()", &[])
        .unwrap()
        .get(0);
    let mut pooled = store.pool.get().unwrap();
    let pool_pid: i32 = pooled
        .query_one("SELECT pg_backend_pid()", &[])
        .unwrap()
        .get(0);
    assert_ne!(fence_pid, pool_pid);
    drop(r2d2::PooledConnection::take(pooled));
    store.table_counts().unwrap();
    assert!(matches!(
        MigrationLease::try_acquire(&fixture.url, fixture.options.clone()),
        Err(PostgresOpenError::SchemaBusy)
    ));
    let mut admin = fixture.client();
    assert!(admin
        .query_one("SELECT pg_terminate_backend($1, 5000)", &[&fence_pid])
        .unwrap()
        .get::<_, bool>(0));
    let lease = MigrationLease::try_acquire(&fixture.url, fixture.options.clone()).unwrap();
    let refusal = store.transaction(|tx| {
        tx.execute("INSERT INTO entities(id,entity_type,scope,properties_json,created_at,updated_at) VALUES ('unfenced','Concept','global','{}','now','now')", &[]).map_err(db_error)?;
        Ok(())
    }).unwrap_err();
    assert!(refusal.contains("schema_busy") || refusal.contains("fence_lost"));
    drop(lease);
    assert!(store
        .transaction(|_| Ok(()))
        .unwrap_err()
        .contains("fence_lost"));
    assert_eq!(
        admin
            .query_one(
                &format!(
                    "SELECT count(*) FROM {}.entities WHERE id='unfenced'",
                    fixture.options.schema
                ),
                &[]
            )
            .unwrap()
            .get::<_, i64>(0),
        0
    );
}
