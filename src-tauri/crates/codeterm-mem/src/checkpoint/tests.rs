use super::*;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn busy_acquisition_skips_checkpoint_with_one_warning() {
    let mut acquisitions = 0;
    let mut warnings = 0;
    let result = checkpoint(
        || {
            acquisitions += 1;
            None
        },
        || warnings += 1,
    );
    assert_eq!(result.unwrap(), (1, -1, -1));
    assert_eq!(acquisitions, 1);
    assert_eq!(warnings, 1);
}

#[test]
fn shutdown_checkpoint_returns_while_connection_holder_keeps_lock() {
    let mem = MemManager::open_in_memory().unwrap();
    let held = mem.conn.lock();
    let worker_mem = mem.clone();
    let (reply, response) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let _ = reply.send(worker_mem.checkpoint_and_close());
    });

    let result = response
        .recv_timeout(Duration::from_secs(1))
        .expect("checkpoint waited for the connection holder");
    assert_eq!(result.unwrap(), (1, -1, -1));
    assert!(mem.conn.try_lock().is_none());
    drop(held);
    worker.join().unwrap();
}

#[test]
fn available_connection_returns_checkpoint_row_without_warning() {
    let mem = MemManager::open_in_memory().unwrap();
    let expected = mem
        .conn
        .lock()
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .unwrap();
    let actual = checkpoint(
        || mem.conn.try_lock(),
        || panic!("available connection was reported busy"),
    )
    .unwrap();
    assert_eq!(actual, expected);
}
