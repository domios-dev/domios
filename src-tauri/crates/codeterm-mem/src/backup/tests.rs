use super::*;
use std::sync::mpsc;

fn fixture(data: &Path) {
    fs::create_dir_all(data.join("mem/blobs")).unwrap();
    fs::create_dir_all(data.join("storage/nested")).unwrap();
    fs::write(data.join("mem/blobs/body.txt"), b"memory body\0\xff").unwrap();
    fs::write(data.join("storage/nested/item.bin"), b"storage item\0\xfe").unwrap();
    let db = Connection::open(data.join("mem/knowledge.db")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE items(id INTEGER PRIMARY KEY, body BLOB); INSERT INTO items VALUES(1, x'616263');").unwrap();
}

#[test]
fn online_sqlite_backup_is_consistent_with_a_concurrent_writer() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.db");
    let db = Connection::open(&source).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE items(id INTEGER PRIMARY KEY, body BLOB); WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<512) INSERT INTO items SELECT x, zeroblob(4096) FROM n;").unwrap();
    let (write, commands) = mpsc::channel();
    let (acknowledge, acknowledgments) = mpsc::channel();
    let writer_path = source.clone();
    let writer = std::thread::spawn(move || {
        let connection = Connection::open(writer_path).unwrap();
        while commands.recv().unwrap() {
            connection
                .execute("INSERT INTO items(body) VALUES(zeroblob(4096))", [])
                .unwrap();
            acknowledge.send(()).unwrap();
        }
    });
    let mut writes = 0;
    let target = root.path().join("copy.db");
    snapshot_with_progress(&source, &target, || {
        if writes == 0 {
            write.send(true).unwrap();
            acknowledgments
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            writes += 1;
        }
    })
    .unwrap();
    write.send(false).unwrap();
    writer.join().unwrap();
    assert_eq!(writes, 1);
    let copy = Connection::open(target).unwrap();
    assert_eq!(
        copy.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(
        copy.query_row("SELECT count(*) FROM items", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        513
    );
}

#[test]
fn restore_refuses_daemon_ownership_and_restores_byte_equal_files_when_stopped() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    fixture(&data);
    let archive = create(&data, &root.path().join("backups")).unwrap();
    assert!(archive.join("manifest.json").is_file());
    assert!(archive.join("mem/knowledge.db").is_file());
    let identity = ResolvedInstancePaths::resolve(&data, &data.join("config.toml")).unwrap();
    let registry = root.path().join("owners");
    let daemon = PathOwnership::try_acquire(identity.clone(), &registry).unwrap();
    assert!(restore(&archive, identity.clone(), &registry)
        .unwrap_err()
        .to_string()
        .contains("stopped daemon"));
    assert_eq!(
        fs::read(data.join("storage/nested/item.bin")).unwrap(),
        b"storage item\0\xfe"
    );
    drop(daemon);
    fs::write(data.join("storage/nested/item.bin"), b"changed").unwrap();
    fs::write(data.join("storage/extra.bin"), b"stale").unwrap();
    let previous = restore(&archive, identity, &registry).unwrap();
    assert_eq!(
        fs::read(data.join("mem/knowledge.db")).unwrap(),
        fs::read(archive.join("mem/knowledge.db")).unwrap()
    );
    assert_eq!(
        fs::read(data.join("mem/blobs/body.txt")).unwrap(),
        b"memory body\0\xff"
    );
    assert_eq!(
        fs::read(data.join("storage/nested/item.bin")).unwrap(),
        b"storage item\0\xfe"
    );
    assert!(!data.join("storage/extra.bin").exists());
    assert_eq!(
        fs::read(previous.join("storage/nested/item.bin")).unwrap(),
        b"changed"
    );
}

#[test]
fn corrupt_archive_never_changes_destination() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    fixture(&data);
    let archive = create(&data, &root.path().join("backups")).unwrap();
    fs::write(archive.join("storage/nested/item.bin"), b"tampered").unwrap();
    let identity = ResolvedInstancePaths::resolve(&data, &data.join("config.toml")).unwrap();
    assert!(restore(&archive, identity, &root.path().join("owners")).is_err());
    assert_eq!(
        fs::read(data.join("storage/nested/item.bin")).unwrap(),
        b"storage item\0\xfe"
    );
}

#[cfg(unix)]
#[test]
fn backup_refuses_storage_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    fixture(&data);
    std::os::unix::fs::symlink(root.path(), data.join("storage/escape")).unwrap();
    assert!(create(&data, &root.path().join("backups")).is_err());
    assert_eq!(
        fs::read_dir(root.path().join("backups")).unwrap().count(),
        0
    );
}
