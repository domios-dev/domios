use super::*;

fn fail(conn: &Connection) -> MemStoreResult<()> {
    conn.execute_batch(
        "CREATE TABLE should_rollback(id INTEGER); INSERT INTO missing_table VALUES(1)",
    )?;
    Ok(())
}

#[test]
fn versioned_step_failure_rolls_back_schema_and_version() {
    let conn = Connection::open_in_memory().unwrap();
    run_steps(&conn, &[baseline]).unwrap();
    let schema: String = conn
        .query_row("SELECT group_concat(sql) FROM sqlite_master", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(run_steps(&conn, &[baseline, fail]).is_err());
    assert_eq!(
        conn.query_row("SELECT version FROM mem_schema_version", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT group_concat(sql) FROM sqlite_master", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        schema
    );
    let empty = Connection::open_in_memory().unwrap();
    assert!(run_steps(&empty, &[fail]).is_err());
    assert_eq!(
        empty
            .query_row("SELECT count(*) FROM sqlite_master", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn versioned_author_columns_roll_back_when_a_later_column_fails() {
    let conn = Connection::open_in_memory().unwrap();
    run_steps(&conn, &[baseline]).unwrap();
    conn.execute_batch("ALTER TABLE mem_items ADD COLUMN author_uid TEXT")
        .unwrap();
    let schema: String = conn
        .query_row("SELECT group_concat(sql) FROM sqlite_master", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(run(&conn).is_err());
    assert_eq!(
        conn.query_row("SELECT version FROM mem_schema_version", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT group_concat(sql) FROM sqlite_master", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        schema
    );
}

struct TempDb {
    path: std::path::PathBuf,
}

impl TempDb {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "codeterm-mem-store-{tag}-{}-{id}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        Self { path }
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(format!("{}-wal", self.path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", self.path.display()));
    }
}

#[test]
fn already_migrated_run_takes_no_write_lock() {
    let db = TempDb::new("no-write-lock");
    {
        let conn = Connection::open(&db.path).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        run(&conn).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
    }

    let ro = Connection::open_with_flags(
        &db.path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    ro.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
    ro.execute_batch("PRAGMA query_only=ON;").unwrap();
    run(&ro).expect("current schema needs no write lock on a read-only connection");

    // Hold a RESERVED write lock on another connection; run() on an already-current DB
    // must still succeed immediately without waiting for or acquiring a write lock.
    let writer = Connection::open(&db.path).unwrap();
    let tx = Transaction::new_unchecked(&writer, TransactionBehavior::Immediate).unwrap();
    let reader = Connection::open(&db.path).unwrap();
    reader
        .busy_timeout(std::time::Duration::from_millis(50))
        .unwrap();
    run(&reader).expect("current schema must not contend with an active write transaction");
    tx.commit().unwrap();
}

#[test]
fn concurrent_migrations_and_readonly_opens_never_lock_fail() {
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::Duration;

    let db = TempDb::new("concurrent");
    let workers = 8usize;
    let barrier = Arc::new(Barrier::new(workers));

    // Phase 1: all 8 connections race to migrate a brand-new database concurrently.
    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let path = db.path.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || -> MemStoreResult<()> {
            let conn = Connection::open(&path)?;
            conn.busy_timeout(Duration::from_secs(5))?;
            barrier.wait();
            run(&conn)
        }));
    }
    for handle in handles {
        handle
            .join()
            .expect("migration thread panicked")
            .expect("concurrent migration must succeed inside busy_timeout");
    }

    {
        let conn = Connection::open(&db.path).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(current_version(&conn).unwrap(), SCHEMA_VERSION);
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
    }

    // Phase 2: while a writer holds BEGIN IMMEDIATE, concurrent read-only queries and
    // writable migrations::run opens must all succeed without database-is-locked errors.
    let (locked_tx, locked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer_path = db.path.clone();
    let writer_handle = std::thread::spawn(move || -> MemStoreResult<()> {
        let conn = Connection::open(&writer_path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let tx = Transaction::new_unchecked(&conn, TransactionBehavior::Immediate)?;
        locked_tx.send(()).expect("signal write lock acquired");
        release_rx.recv().expect("wait for release signal");
        tx.execute(
            "INSERT INTO entities (id, entity_type, scope, properties_json, created_at, updated_at)
             VALUES ('topic_concurrent', 'Topic', 'global', '{}', '2026-01-01', '2026-01-01')",
            [],
        )?;
        tx.commit()?;
        Ok(())
    });

    locked_rx.recv().expect("writer acquired lock");
    let phase2_barrier = Arc::new(Barrier::new(8));
    let mut phase2 = Vec::with_capacity(8);
    for index in 0..8 {
        let path = db.path.clone();
        let barrier = Arc::clone(&phase2_barrier);
        phase2.push(std::thread::spawn(move || -> MemStoreResult<()> {
            barrier.wait();
            if index % 2 == 0 {
                let conn = Connection::open_with_flags(
                    &path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                        | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )?;
                conn.busy_timeout(Duration::from_secs(5))?;
                conn.execute_batch("PRAGMA query_only=ON;")?;
                let _: i64 = conn.query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))?;
            } else {
                let conn = Connection::open(&path)?;
                conn.busy_timeout(Duration::from_secs(5))?;
                run(&conn)?;
            }
            Ok(())
        }));
    }
    for handle in phase2 {
        handle
            .join()
            .expect("concurrent reader/opener thread panicked")
            .expect("concurrent open/read must never fail with database is locked");
    }
    release_tx.send(()).expect("release writer");
    writer_handle
        .join()
        .expect("writer thread panicked")
        .expect("writer commit must succeed");
}
