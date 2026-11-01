use super::*;
use std::cell::Cell;

fn exercise(url: &str, options: PostgresOptions) {
    let store = open(url, options.clone()).unwrap();
    contract::save_replace_history(store.as_ref());
    contract::fts_hit(store.as_ref());
    let ctx = WriteContext::system("runtime-boundary");
    let id = store
        .save(&ctx, &contract::item("runtime boundary"), false)
        .unwrap()
        .id;
    assert_eq!(store.get(&id).unwrap().unwrap().title, "runtime boundary");
    assert!(store
        .pending_extraction(usize::MAX)
        .unwrap()
        .iter()
        .any(|row| row.0 == id));
    assert!(store.bump_evidence(&ctx, &id).unwrap());
    let concrete = store
        .fixture_backend()
        .downcast_ref::<PostgresMemStore>()
        .unwrap();
    assert_eq!(
        concrete
            .transaction(|tx| Ok(tx
                .query_one("SELECT evidence_count FROM mem_items WHERE id=$1", &[&id])
                .map_err(db_error)?
                .get::<_, i64>(0)))
            .unwrap(),
        2
    );
    store.clear_shadow(&ctx, true).unwrap();
    assert!(store.sync_status(Duration::from_secs(1)).is_some());
    let visited = Cell::new(false);
    store
        .with_export_snapshot(&mut |snapshot| {
            assert!(!snapshot.table_counts()?.is_empty());
            assert!(!snapshot.export_page(None, 1)?.rows.is_empty());
            visited.set(true);
            Ok(())
        })
        .unwrap();
    assert!(visited.get());
    let readonly = open(url, options.clone().read_only()).unwrap();
    assert!(readonly.get(&id).unwrap().is_some());
    assert!(readonly
        .save(&ctx, &contract::item("refused"), false)
        .unwrap_err()
        .to_string()
        .contains("postgres_memory_read_only (25006)"));
    drop(readonly);
    let concrete = store
        .fixture_backend()
        .downcast_ref::<PostgresMemStore>()
        .unwrap();
    concrete.vacuum_precompact().unwrap();
    let result: Result<(), String> = concrete.transaction(|tx| {
        items::insert_in(tx, &ctx, &contract::item("rolled back"), None)?;
        Err("rollback fixture".into())
    });
    assert_eq!(result.unwrap_err(), "rollback fixture");
    drop(store);

    let lease = MigrationLease::try_acquire(url, options.clone()).unwrap();
    let claimed = lease.open(false).unwrap();
    assert!(claimed.get(&id).unwrap().is_some());
    claimed.clear_shadow(&ctx, true).unwrap();
    claimed
        .with_export_snapshot(&mut |snapshot| {
            assert!(!snapshot.table_counts()?.is_empty());
            Ok(())
        })
        .unwrap();
    claimed
        .fixture_backend()
        .downcast_ref::<PostgresMemStore>()
        .unwrap()
        .vacuum_precompact()
        .unwrap();
    assert!(matches!(
        open(url, options),
        Err(PostgresOpenError::SchemaBusy)
    ));
    drop(lease);
    assert!(claimed.get(&id).unwrap().is_some());
    drop(claimed);
}

fn fixture() -> Fixture {
    let mut fixture = Fixture::new();
    fixture.store.take();
    fixture
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn postgres_from_plain_thread() {
    let fixture = fixture();
    std::thread::scope(|scope| {
        scope
            .spawn(|| exercise(&fixture.url, fixture.options.clone()))
            .join()
            .unwrap();
    });
}

#[test]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
fn postgres_from_current_thread_runtime() {
    let fixture = fixture();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            exercise(&fixture.url, fixture.options.clone());
        });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires disposable pgvector and DOMIOS_TEST_POSTGRES_URL"]
async fn postgres_from_multi_thread_task() {
    let fixture = tokio::task::spawn_blocking(fixture).await.unwrap();
    let url = fixture.url.clone();
    let options = fixture.options.clone();
    tokio::spawn(async move {
        exercise(&url, options);
    })
    .await
    .unwrap();
    tokio::task::spawn_blocking(move || drop(fixture))
        .await
        .unwrap();
}
