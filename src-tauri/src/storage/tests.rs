use super::StorageError as E;
use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::time::Duration;

mod assets;

/// Failure bound around event waits only; elapsed time never signals readiness.
const DEADLINE: Duration = Duration::from_secs(30);

fn temp_root() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp root")
}

fn open(root: &Path) -> Storage {
    Storage::open(root, StorageConfig::default()).expect("open storage")
}

fn config(change: fn(&mut StorageConfig)) -> StorageConfig {
    let mut config = StorageConfig::default();
    change(&mut config);
    config
}

fn done<T>(completion: Completion<T>) -> T {
    completion.wait(DEADLINE).expect("storage operation")
}

fn failed<T: std::fmt::Debug>(completion: Completion<T>) -> StorageError {
    completion.wait(DEADLINE).expect_err("operation must fail")
}

/// Outcome already settled at submission, such as an admission refusal.
fn immediate<T>(completion: Completion<T>) -> Option<Result<T, StorageError>> {
    completion.try_take().ok()
}

fn close(storage: Storage) -> ShutdownReceipt {
    done(storage.shutdown())
}

fn id(text: &str) -> WorkspaceId {
    WorkspaceId::new(text).expect("workspace id")
}

fn role(text: &str) -> AssetRole {
    AssetRole::new(text).expect("asset role")
}

fn record(text: &str) -> WorkspaceRecord {
    WorkspaceRecord {
        cwd: Some(format!("/projects/{text}")),
        protected: text == "a",
        task_key_prefix: Some(text.to_ascii_uppercase()),
        task_key_next: 7,
        ..WorkspaceRecord::new(id(text), format!("Workspace {text}"))
    }
}

fn absent(record: WorkspaceRecord) -> WorkspaceUpsert {
    WorkspaceUpsert {
        record,
        expect: Expect::Absent,
    }
}

fn create(storage: &Storage, names: &[&str]) -> CommitReceipt {
    let batch = names.iter().map(|name| absent(record(name))).collect();
    done(storage.workspaces().upsert(batch)).receipt
}

fn insert(storage: &Storage, name: &str) -> Completion<Committed<()>> {
    storage.workspaces().upsert(vec![absent(record(name))])
}

fn rename(storage: &Storage, name: &str, expect: Revision, to: &str) -> Completion<Committed<()>> {
    storage
        .workspaces()
        .rename(id(name), expect, to.to_string())
}

fn attach(
    storage: &Storage,
    name: &str,
    expect: Revision,
    slot: &str,
    bytes: &[u8],
) -> Completion<Committed<AssetRef>> {
    storage
        .workspaces()
        .attach_asset(id(name), expect, role(slot), bytes.to_vec())
}

fn asset_bytes(storage: &Storage, name: &str, slot: &str) -> Option<Vec<u8>> {
    done(storage.workspaces().asset(id(name), role(slot))).value
}

fn inventory(storage: &Storage) -> WorkspaceInventory {
    done(storage.workspaces().list()).value
}

fn ids(inventory: &WorkspaceInventory) -> Vec<String> {
    inventory
        .workspaces
        .iter()
        .map(|w| w.record.id.as_str().to_string())
        .collect()
}

fn stored<'a>(inventory: &'a WorkspaceInventory, name: &str) -> &'a StoredWorkspace {
    let found = inventory
        .workspaces
        .iter()
        .find(|w| w.record.id.as_str() == name);
    found.expect("stored workspace")
}

/// (committed, dangling, orphan blobs, staging files, pending cleanups)
fn audit(storage: &Storage) -> (u64, usize, usize, u64, usize) {
    let audit = done(storage.assets().audit());
    let pending = audit.pending_cleanup.len();
    (
        audit.committed,
        audit.dangling.len(),
        audit.orphan_blobs.len(),
        audit.temp_files,
        pending,
    )
}

fn held(hold: Result<Hold, StorageError>) -> Hold {
    let hold = hold.expect("writer hold admitted");
    hold.wait_held(DEADLINE).expect("writer reached the hold");
    hold
}

fn is_foreign_key(error: &StorageError) -> bool {
    matches!(
        error,
        E::Constraint {
            kind: ConstraintKind::ForeignKey,
            ..
        }
    )
}

fn storage_dir(root: &Path) -> PathBuf {
    Storage::database_path(root)
        .parent()
        .expect("storage dir")
        .to_path_buf()
}

#[test]
fn storage_contract_config_bounds_are_explicit() {
    let defaults = StorageConfig::default();
    assert_eq!(
        (defaults.mailbox_commands, defaults.mailbox_bytes),
        (128, 8 * 1024 * 1024)
    );
    assert_eq!((defaults.read_connections, MAX_READ_CONNECTIONS), (2, 2));
    assert!(defaults.max_asset_bytes <= defaults.mailbox_bytes);
    let root = temp_root();
    for invalid in [
        config(|c| c.read_connections = 0),
        config(|c| c.read_connections = 3),
        config(|c| c.mailbox_commands = 0),
        config(|c| c.max_asset_bytes = 9 * 1024 * 1024),
        config(|c| c.busy_timeout = Duration::ZERO),
    ] {
        let refused = Storage::open(root.path(), invalid).err();
        assert!(
            matches!(refused, Some(E::InvalidConfig { .. })),
            "{refused:?}"
        );
    }
    close(open(root.path()));
}

#[test]
fn storage_contract_effective_pragmas_are_verified() {
    let root = temp_root();
    let storage = open(root.path());
    let settings = storage.effective_settings();
    let writer = &settings.writer;
    assert_eq!(
        (writer.journal_mode.as_str(), writer.synchronous),
        ("wal", Synchronous::Full)
    );
    assert!(writer.foreign_keys && !writer.query_only);
    assert_eq!(writer.busy_timeout, Duration::from_secs(5));
    assert_eq!(settings.readers.len(), 2);
    for reader in &settings.readers {
        assert_eq!(
            (reader.journal_mode.as_str(), reader.synchronous),
            ("wal", Synchronous::Full)
        );
        assert!(reader.query_only && reader.foreign_keys);
    }
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY;
    let external =
        rusqlite::Connection::open_with_flags(Storage::database_path(root.path()), flags);
    let external = external.expect("independent reader");
    let mode: String = external
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    drop(external);
    close(storage);
}

#[test]
fn storage_contract_workspace_rows_are_incremental_and_ordered() {
    let root = temp_root();
    let storage = open(root.path());
    assert!(inventory(&storage).workspaces.is_empty());
    let created = create(&storage, &["a", "b", "c", "d", "e"]);
    assert_eq!(created.rows_written, 5);
    let listed = inventory(&storage);
    assert_eq!(ids(&listed), ["a", "b", "c", "d", "e"]);
    assert_eq!(stored(&listed, "c").record, record("c"));
    assert!(listed
        .workspaces
        .iter()
        .all(|w| w.revision == created.revision));

    let renamed = done(rename(&storage, "c", created.revision, "renamed")).receipt;
    assert_eq!(renamed.rows_written, 1);
    assert!(renamed.revision > created.revision);
    let after_rename = inventory(&storage);
    for workspace in &after_rename.workspaces {
        let touched = workspace.record.id == id("c");
        let expected = if touched {
            renamed.revision
        } else {
            created.revision
        };
        assert_eq!(
            workspace.revision,
            expected,
            "{}",
            workspace.record.id.as_str()
        );
    }
    assert_eq!(stored(&after_rename, "c").record.name, "renamed");

    let swapped = ["a", "d", "c", "b", "e"].map(id).to_vec();
    let reordered = done(
        storage
            .workspaces()
            .reorder(after_rename.order_revision, swapped),
    )
    .receipt;
    assert_eq!(reordered.rows_written, 2, "only displaced rows are written");
    let after_reorder = inventory(&storage);
    assert_eq!(ids(&after_reorder), ["a", "d", "c", "b", "e"]);
    assert_eq!(after_reorder.order_revision, reordered.revision);
    for workspace in &after_reorder.workspaces {
        let before = stored(&after_rename, workspace.record.id.as_str());
        assert_eq!(
            workspace.revision, before.revision,
            "reorder leaves row revisions alone"
        );
    }

    let removed = done(storage.workspaces().remove(id("d"), created.revision)).receipt;
    assert_eq!(removed.rows_written, 1);
    let final_inventory = inventory(&storage);
    assert_eq!(ids(&final_inventory), ["a", "c", "b", "e"]);
    let single = done(storage.workspaces().get(id("c")))
        .value
        .expect("present");
    assert_eq!(single.record.name, "renamed");
    assert!(done(storage.workspaces().get(id("d"))).value.is_none());

    assert_eq!(close(storage).revision, removed.revision);
    let reopened = open(root.path());
    assert_eq!(inventory(&reopened), final_inventory);
    assert_eq!(reopened.revision(), removed.revision);
    close(reopened);
}

#[test]
fn storage_contract_stale_revision_conserves_entire_prior_set() {
    let root = temp_root();
    let storage = open(root.path());
    let created = create(&storage, &["a", "b", "c"]).revision;
    let renamed = done(rename(&storage, "a", created, "a2")).receipt.revision;
    let before = inventory(&storage);
    let committed = storage.revision();

    let changed_b = WorkspaceRecord {
        name: "b2".into(),
        ..record("b")
    };
    let stale_batch = vec![
        WorkspaceUpsert {
            record: changed_b,
            expect: Expect::At(created),
        },
        absent(record("d")),
        WorkspaceUpsert {
            record: record("a"),
            expect: Expect::At(created),
        },
    ];
    let expected = E::StaleRevision {
        entity: "workspace a".into(),
        expected: Some(created),
        actual: Some(renamed),
    };
    assert_eq!(failed(storage.workspaces().upsert(stale_batch)), expected);
    let existing = failed(insert(&storage, "b"));
    assert!(matches!(
        existing,
        E::StaleRevision {
            expected: None,
            actual: Some(_),
            ..
        }
    ));
    let missing = failed(rename(&storage, "zz", created, "x"));
    assert!(matches!(missing, E::StaleRevision { actual: None, .. }));
    assert!(matches!(
        failed(rename(&storage, "a", created, "x")),
        E::StaleRevision { .. }
    ));
    let order = before.order_revision;
    for invalid in [vec!["a", "b"], vec!["a", "b", "b"], vec!["a", "b", "zz"]] {
        let invalid = invalid.into_iter().map(id).collect();
        let refused = failed(storage.workspaces().reorder(order, invalid));
        assert!(matches!(refused, E::Invalid { .. }), "{refused:?}");
    }
    let repeated = vec![absent(record("x")), absent(record("x"))];
    assert!(matches!(
        failed(storage.workspaces().upsert(repeated)),
        E::Invalid { .. }
    ));
    assert_eq!(inventory(&storage), before);
    assert_eq!(storage.revision(), committed);

    create(&storage, &["d"]);
    let current = inventory(&storage);
    let reversed = ["d", "c", "b", "a"].map(id).to_vec();
    let stale_order = failed(storage.workspaces().reorder(order, reversed));
    assert!(matches!(stale_order, E::StaleRevision { .. }));
    assert_eq!(inventory(&storage), current);
    close(storage);
}

#[test]
fn storage_contract_foreign_key_failure_rolls_back_only_its_operation() {
    let root = temp_root();
    let storage = open(root.path());
    let created = create(&storage, &["a", "b", "c"]).revision;
    let b_revision = done(attach(&storage, "b", created, "icon", b"payload"))
        .receipt
        .revision;
    let before = inventory(&storage);
    let refused = failed(storage.workspaces().remove(id("b"), b_revision));
    assert!(is_foreign_key(&refused), "{refused:?}");
    assert_eq!(inventory(&storage), before);

    let hold = held(storage.hold_writer());
    let rename_a = rename(&storage, "a", created, "a2");
    let remove_b = storage.workspaces().remove(id("b"), b_revision);
    let rename_c = rename(&storage, "c", created, "c2");
    hold.release();
    let (a, c) = (done(rename_a).receipt, done(rename_c).receipt);
    assert!(is_foreign_key(&failed(remove_b)));
    assert_eq!(
        (a.timings.group_size, c.timings.group_size),
        (3, 3),
        "one grouped transaction"
    );
    assert!(c.revision > a.revision);
    let after = inventory(&storage);
    assert_eq!(ids(&after), ["a", "b", "c"]);
    let names = (
        stored(&after, "a").record.name.as_str(),
        stored(&after, "c").record.name.as_str(),
    );
    assert_eq!(names, ("a2", "c2"));
    assert_eq!(stored(&after, "b"), stored(&before, "b"));
    assert_eq!(audit(&storage), (1, 0, 0, 0, 0));
    close(storage);
}

#[test]
fn storage_contract_injected_commit_failure_conserves_state() {
    let root = temp_root();
    let storage = open(root.path());
    let created = create(&storage, &["a", "b"]).revision;
    let before = inventory(&storage);
    let committed = storage.revision();
    done(storage.inject_fault(Fault::FailNextCommit));
    assert_eq!(
        failed(rename(&storage, "a", created, "lost")),
        E::Injected { point: "commit" }
    );
    assert_eq!(inventory(&storage), before);
    assert_eq!(storage.revision(), committed);
    assert!(storage.metrics().failed_operations >= 1);

    let kept = done(rename(&storage, "a", created, "kept")).receipt;
    assert!(kept.revision > committed);
    close(storage);
    let reopened = open(root.path());
    assert_eq!(stored(&inventory(&reopened), "a").record.name, "kept");
    close(reopened);
}

#[test]
fn storage_contract_database_full_rolls_back_batch() {
    let root = temp_root();
    let storage = open(root.path());
    create(&storage, &["seed"]);
    let before = inventory(&storage);
    let wide: Vec<WorkspaceUpsert> = (0..64)
        .map(|index| {
            absent(WorkspaceRecord {
                name: "x".repeat(1000),
                ..record(&format!("w{index}"))
            })
        })
        .collect();
    done(storage.inject_fault(Fault::DatabaseFull));
    assert_eq!(
        failed(storage.workspaces().upsert(wide.clone())),
        E::DatabaseFull
    );
    assert_eq!(inventory(&storage), before);
    done(storage.inject_fault(Fault::ClearDatabaseFull));
    done(storage.workspaces().upsert(wide));
    assert_eq!(
        inventory(&storage).workspaces.len(),
        before.workspaces.len() + 64
    );
    close(storage);
}

#[test]
fn storage_contract_held_writer_serves_committed_reads_and_bounds_admission() {
    let root = temp_root();
    let storage = Storage::open(root.path(), config(|c| c.mailbox_commands = 4)).expect("open");
    create(&storage, &["a"]);
    let committed = storage.revision();
    let hold = held(storage.hold_write_transaction());

    let accepted: Vec<_> = (0..4)
        .map(|index| insert(&storage, &format!("q{index}")))
        .collect();
    let refused = immediate(insert(&storage, "q4"));
    let full = matches!(
        refused,
        Some(Err(E::Backpressure {
            queued_commands: 4,
            limit_commands: 4,
            ..
        }))
    );
    assert!(full, "{refused:?}");
    let read = done(storage.workspaces().list());
    assert_eq!(
        (read.revision, ids(&read.value)),
        (committed, vec!["a".to_string()])
    );
    assert_eq!(
        read.value.workspaces[0].record.name, "Workspace a",
        "uncommitted state stays invisible"
    );
    assert!(
        accepted.iter().all(|completion| !completion.is_settled()),
        "no early acknowledgment"
    );
    assert_eq!(storage.revision(), committed);
    let watcher = storage.committed_after(committed);
    assert!(!watcher.is_settled());

    hold.release();
    for completion in accepted {
        done(completion);
    }
    assert!(done(watcher) > committed);
    let after = inventory(&storage);
    assert_eq!(ids(&after), ["a", "q0", "q1", "q2", "q3"]);
    assert_eq!(
        stored(&after, "a").record.name,
        "Workspace a",
        "the held change rolled back"
    );
    assert!(storage.metrics().rejected_admissions >= 1);
    close(storage);
}

#[test]
fn storage_contract_mailbox_byte_bound_refuses_without_discard() {
    let root = temp_root();
    let bounded = config(|c| {
        c.mailbox_bytes = 64 * 1024;
        c.max_asset_bytes = 48 * 1024;
    });
    let storage = Storage::open(root.path(), bounded).expect("open");
    let revision = create(&storage, &["a"]).revision;
    let hold = held(storage.hold_writer());
    let first = attach(&storage, "a", revision, "one", &[1; 40 * 1024]);
    let second = immediate(attach(&storage, "a", revision, "two", &[2; 40 * 1024]));
    assert!(
        matches!(
            second,
            Some(Err(E::Backpressure {
                queued_commands: 1,
                ..
            }))
        ),
        "{second:?}"
    );
    let oversized = immediate(attach(&storage, "a", revision, "three", &[3; 100 * 1024]));
    assert!(
        matches!(oversized, Some(Err(E::TooLarge { .. }))),
        "{oversized:?}"
    );
    assert!(!first.is_settled());
    hold.release();

    let attached = done(first);
    assert_eq!(attached.value.byte_len, 40 * 1024);
    assert_eq!(asset_bytes(&storage, "a", "one"), Some(vec![1; 40 * 1024]));
    let above_bound = failed(attach(
        &storage,
        "a",
        attached.receipt.revision,
        "four",
        &[4; 50 * 1024],
    ));
    assert!(matches!(above_bound, E::TooLarge { .. }));
    assert_eq!(
        audit(&storage),
        (1, 0, 0, 0, 0),
        "no file is written for a refused asset"
    );
    close(storage);
}

#[test]
fn storage_contract_owner_alias_and_instance_isolation() {
    let base = temp_root();
    let root = base.path().join("instance");
    let storage = open(&root);
    let reopen = |path: &Path| Storage::open(path, StorageConfig::default()).err();
    assert!(matches!(reopen(&root), Some(E::AlreadyOpen { .. })));
    assert!(matches!(
        reopen(&root.join("..").join("instance")),
        Some(E::AlreadyOpen { .. })
    ));
    #[cfg(unix)]
    let alias = {
        let link = base.path().join("alias");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        link
    };
    #[cfg(windows)]
    let alias = PathBuf::from(root.to_string_lossy().to_uppercase());
    let refused = reopen(&alias);
    assert!(
        matches!(refused, Some(E::AlreadyOpen { .. } | E::Locked { .. })),
        "{refused:?}"
    );

    let foreign = base.path().join("foreign");
    close(open(&foreign));
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(Storage::lock_path(&foreign));
    let lock = lock.expect("lock file");
    lock.try_lock()
        .expect("simulated foreign owner takes the OS lock");
    assert!(matches!(reopen(&foreign), Some(E::Locked { .. })));
    drop(lock);
    close(open(&foreign));

    let second = open(&base.path().join("other"));
    create(&storage, &["shared-id"]);
    let theirs = WorkspaceRecord {
        name: "other".into(),
        ..record("shared-id")
    };
    done(second.workspaces().upsert(vec![absent(theirs.clone())]));
    assert_eq!(
        inventory(&storage).workspaces[0].record,
        record("shared-id")
    );
    assert_eq!(inventory(&second).workspaces[0].record, theirs);
    close(second);
    close(storage);
    let reopened = open(&root);
    assert_eq!(ids(&inventory(&reopened)), ["shared-id"]);
    close(reopened);
}

fn assert_refused(root: &Path, expected: fn(&StorageError) -> bool) {
    let database = Storage::database_path(root);
    let before = fs::read(&database).ok();
    for _ in 0..2 {
        let refused = Storage::open(root, StorageConfig::default()).err();
        assert!(
            refused.as_ref().is_some_and(expected),
            "{}: {refused:?}",
            root.display()
        );
    }
    assert_eq!(
        fs::read(&database).ok(),
        before,
        "a refused database is never rewritten"
    );
}

#[test]
fn storage_contract_malformed_and_incompatible_databases_are_refused() {
    let base = temp_root();
    let fixture = |name: &str, bytes: Option<&[u8]>| {
        let root = base.path().join(name);
        fs::create_dir_all(storage_dir(&root)).unwrap();
        if let Some(bytes) = bytes {
            fs::write(Storage::database_path(&root), bytes).unwrap();
        }
        root
    };
    let garbage = fixture("garbage", Some(&[0x5a; 8192][..]));
    assert_refused(&garbage, |e| matches!(e, E::Corrupt { .. }));
    let empty = fixture("empty", Some(&[][..]));
    assert_refused(&empty, |e| {
        matches!(
            e,
            E::Incompatible {
                application_id: 0,
                schema_version: 0,
                ..
            }
        )
    });

    let foreign = fixture("foreign", None);
    let conn = rusqlite::Connection::open(Storage::database_path(&foreign)).unwrap();
    conn.execute_batch("CREATE TABLE t(x); PRAGMA application_id = 7; PRAGMA user_version = 1;")
        .unwrap();
    drop(conn);
    assert_refused(&foreign, |e| {
        matches!(
            e,
            E::Incompatible {
                application_id: 7,
                ..
            }
        )
    });

    let newer = base.path().join("newer");
    close(open(&newer));
    let conn = rusqlite::Connection::open(Storage::database_path(&newer)).unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();
    drop(conn);
    assert_refused(&newer, |e| {
        matches!(
            e,
            E::Incompatible {
                schema_version: 99,
                ..
            }
        )
    });

    let damaged = base.path().join("damaged");
    close(open(&damaged));
    let mut bytes = fs::read(Storage::database_path(&damaged)).unwrap();
    assert!(
        bytes.len() > 8192,
        "fixture spans more than the header page"
    );
    bytes[4096..8192].fill(0xa5);
    fs::write(Storage::database_path(&damaged), &bytes).unwrap();
    assert_refused(&damaged, |e| matches!(e, E::Corrupt { .. }));

    let orphan_wal = fixture("orphan-wal", None);
    fs::write(storage_dir(&orphan_wal).join("state.db-wal"), b"wal").unwrap();
    assert_refused(&orphan_wal, |e| matches!(e, E::InconsistentRoot { .. }));

    let orphan_assets = fixture("orphan-assets", None);
    let blobs = storage_dir(&orphan_assets).join("assets").join("blobs");
    fs::create_dir_all(&blobs).unwrap();
    fs::write(blobs.join("0".repeat(32)), b"asset").unwrap();
    assert_refused(&orphan_assets, |e| matches!(e, E::InconsistentRoot { .. }));
}

#[test]
fn storage_contract_backup_while_wal_active_restores_consistently() {
    let base = temp_root();
    let root = base.path().join("live");
    let storage = open(&root);
    let created = create(&storage, &["a", "b"]).revision;
    done(attach(&storage, "b", created, "blob", &[7; 4096]));
    let renamed = done(rename(&storage, "a", created, "after-attach"))
        .receipt
        .revision;
    let expected = inventory(&storage);

    let image = base.path().join("image");
    let receipt = done(storage.backup_to(&image));
    assert_eq!(receipt.revision, storage.revision());
    assert_eq!((receipt.assets, receipt.asset_bytes), (1, 4096));
    assert!(matches!(
        failed(storage.backup_to(&image)),
        E::Invalid { .. }
    ));

    let restored_root = base.path().join("restored");
    let restored = Storage::restore_backup(&image, &restored_root).expect("restore");
    assert_eq!(restored.revision, receipt.revision);
    let occupied = Storage::restore_backup(&image, &restored_root).err();
    assert!(
        matches!(occupied, Some(E::RestoreTargetOccupied { .. })),
        "{occupied:?}"
    );
    let live = Storage::restore_backup(&image, &root).err();
    assert!(matches!(live, Some(E::AlreadyOpen { .. })), "{live:?}");

    let copy = open(&restored_root);
    assert_eq!(inventory(&copy), expected);
    assert_eq!(copy.revision(), receipt.revision);
    assert_eq!(asset_bytes(&copy, "b", "blob"), Some(vec![7; 4096]));
    assert_eq!(audit(&copy), (1, 0, 0, 0, 0));
    done(rename(&copy, "a", renamed, "copy-only"));
    assert_eq!(
        inventory(&storage),
        expected,
        "the restored copy is an independent owner"
    );
    close(copy);
    close(storage);
}

#[test]
fn storage_contract_shutdown_drains_accepted_and_releases_handles() {
    let base = temp_root();
    let root = base.path().join("instance");
    let storage = open(&root);
    create(&storage, &["a"]);
    let hold = held(storage.hold_writer());
    let queued: Vec<_> = ["x", "y", "z"]
        .iter()
        .map(|name| insert(&storage, name))
        .collect();
    let shutdown = storage.shutdown();

    let late_write = immediate(insert(&storage, "late"));
    assert!(matches!(late_write, Some(Err(E::Closed))), "{late_write:?}");
    let late_read = immediate(storage.workspaces().list());
    assert!(matches!(late_read, Some(Err(E::Closed))), "{late_read:?}");
    assert!(queued.iter().all(|completion| !completion.is_settled()));
    assert!(!shutdown.is_settled());
    let observer = storage.shutdown().wait(Duration::ZERO);
    assert!(
        matches!(observer, Err(E::Unconfirmed { .. })),
        "a missed deadline is not a failure"
    );

    hold.release();
    for completion in queued {
        done(completion);
    }
    let receipt = done(shutdown);
    assert_eq!((receipt.drained_writes, receipt.checkpointed), (3, true));
    assert_eq!(receipt.revision, storage.revision());
    assert_eq!(
        done(storage.shutdown()),
        receipt,
        "later waiters observe the same outcome"
    );

    let sidecars: Vec<String> = fs::read_dir(storage_dir(&root))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| {
            name.ends_with("-wal") || name.ends_with("-shm") || name.ends_with("-journal")
        })
        .collect();
    assert!(sidecars.is_empty(), "{sidecars:?}");
    let moved = base.path().join("moved");
    fs::rename(&root, &moved).expect("no open handle pins the released root");
    drop(storage);
    let reopened = open(&moved);
    assert_eq!(ids(&inventory(&reopened)), ["a", "x", "y", "z"]);
    close(reopened);
}

#[test]
fn storage_contract_shutdown_admission_race_settles_every_submission() {
    const SUBMITTERS: usize = 4;
    const PER_SUBMITTER: usize = 16;
    let root = temp_root();
    let storage = open(root.path());
    let barrier = Arc::new(Barrier::new(SUBMITTERS + 1));
    let submitters: Vec<_> = (0..SUBMITTERS)
        .map(|thread| {
            let (storage, barrier) = (storage.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let names = (0..PER_SUBMITTER).map(|index| format!("w{thread}-{index}"));
                names
                    .map(|name| (name.clone(), insert(&storage, &name)))
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    barrier.wait();
    let shutdown = storage.shutdown();
    let mut acknowledged = Vec::new();
    for submitter in submitters {
        for (name, completion) in submitter.join().expect("submitter") {
            match completion.wait(DEADLINE) {
                Ok(_) => acknowledged.push(name),
                Err(E::Closed) => {}
                Err(other) => panic!("{name} settled as {other:?}"),
            }
        }
    }
    done(shutdown);
    drop(storage);
    let reopened = open(root.path());
    let mut durable = ids(&inventory(&reopened));
    durable.sort();
    acknowledged.sort();
    assert_eq!(
        durable, acknowledged,
        "every acknowledgment is durable; every refusal left nothing"
    );
    close(reopened);
}

#[test]
fn storage_contract_owner_threads_refuse_reentrant_calls() {
    let base = temp_root();
    let storage = open(&base.path().join("owner"));
    let probe = done(storage.probe_reentrancy(&base.path().join("other")));
    let reentrant = Some(E::Reentrant);
    let expected = ReentrancyProbe {
        read: reentrant.clone(),
        wait: reentrant.clone(),
        shutdown: reentrant.clone(),
        open: reentrant,
    };
    assert_eq!(probe, expected);
    create(&storage, &["after-probe"]);
    assert_eq!(ids(&inventory(&storage)), ["after-probe"]);
    close(storage);
}

#[tokio::test(flavor = "current_thread")]
async fn storage_contract_completion_resolves_on_async_runtime() {
    let root = temp_root();
    let path = root.path().to_path_buf();
    let opened =
        tokio::task::spawn_blocking(move || Storage::open(&path, StorageConfig::default()));
    let storage = opened.await.expect("open task").expect("open storage");
    let committed = insert(&storage, "async")
        .await
        .expect("upsert")
        .receipt
        .revision;
    let read = storage.workspaces().list().await.expect("list");
    assert_eq!(
        (read.revision, ids(&read.value)),
        (committed, vec!["async".to_string()])
    );
    let watcher = storage.committed_after(committed);
    let renamed = rename(&storage, "async", committed, "renamed")
        .await
        .expect("rename");
    assert_eq!(
        watcher.await.expect("commit event"),
        renamed.receipt.revision
    );
    let receipt = storage.shutdown().await.expect("shutdown");
    assert_eq!(receipt.revision, renamed.receipt.revision);
}

#[test]
fn storage_contract_identifiers_are_confined() {
    let not_hex = "g".repeat(32);
    let upper_hex = "ABCDEF0123456789ABCDEF0123456789";
    for invalid in [
        "../../etc/passwd",
        "NUL",
        upper_hex,
        "0123",
        not_hex.as_str(),
    ] {
        assert!(AssetId::parse(invalid).is_err(), "{invalid}");
    }
    assert!(AssetId::parse(&"0a".repeat(16)).is_ok());
    let long_role = "r".repeat(65);
    for invalid in [
        "",
        "Upper",
        "../x",
        "a/b",
        "a\\b",
        "CON",
        long_role.as_str(),
    ] {
        assert!(AssetRole::new(invalid).is_err(), "{invalid}");
    }
    assert!(WorkspaceId::new("").is_err() && WorkspaceId::new("a\u{0}b").is_err());
    assert!(WorkspaceId::new("x".repeat(257)).is_err());
}

/// Unix symlink or Windows directory junction; `mklink /J` needs no privilege.
fn link_dir(link: &Path, target: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("directory symlink");
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("cmd");
        codeterm_util::process_util::no_window_std(&mut command);
        let output = command
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output();
        let output = output.expect("run mklink");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "junction creation failed: {stderr}"
        );
    }
}

fn unlink_dir(link: &Path) {
    #[cfg(unix)]
    fs::remove_file(link).expect("remove symlink");
    #[cfg(windows)]
    fs::remove_dir(link).expect("remove junction");
}

#[test]
fn storage_contract_linked_storage_directories_are_refused() {
    let base = temp_root();
    let outside = base.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let top = base.path().join("top");
    fs::create_dir_all(&top).unwrap();
    link_dir(&top.join("storage"), &outside);
    let nested = base.path().join("nested");
    close(open(&nested));
    let blobs = storage_dir(&nested).join("assets").join("blobs");
    fs::remove_dir(&blobs).unwrap();
    link_dir(&blobs, &outside);

    for root in [&top, &nested] {
        for _ in 0..2 {
            let refused = Storage::open(root, StorageConfig::default()).err();
            assert!(
                matches!(refused, Some(E::RootInvalid { .. })),
                "{refused:?}"
            );
        }
    }
    let outside_entries = fs::read_dir(&outside).unwrap().count();
    assert_eq!(
        outside_entries, 0,
        "nothing is written through a link or junction"
    );
    unlink_dir(&top.join("storage"));
    unlink_dir(&blobs);
}
