//! storage_migration_ transaction: one all-domain commit gated on typed projections, with
//! rollback before the marker, restart without replay, and committed state as authority.

use super::*;

fn existing_job() -> CronJobRecord {
    job("existing").record
}

/// Enough cron bytes that the import must grow the database file, beside the Sentinels.
fn growing_plan(f: &Fixture) -> ImportPlan {
    let jobs = (0..64)
        .map(|index| {
            let mut wide = job(&format!("wide-{index}"));
            wide.record.task = "x".repeat(1000);
            wide
        })
        .collect();
    let decoder = Fixed {
        cron: CronImport {
            jobs,
            histories: Vec::new(),
        },
        sentinel: decoded_sentinels(),
    };
    ImportPlan::resolve(f.capture(), &decoder, &BTreeMap::new()).expect("plan")
}

#[test]
fn storage_migration_projection_mismatch_rolls_back_before_commit() {
    let f = Fixture::seeded();
    let storage = f.open();

    let error = storage
        .commit_legacy_import_tampered(f.plan(), DEADLINE, ImportTamper::CronDefinition)
        .expect_err("a written row that differs from the original refuses");
    match refusal(error) {
        ImportRefusal::ProjectionMismatch {
            domain: ImportDomain::Cron,
            mismatches,
            ..
        } => assert_eq!(
            mismatches[0],
            ProjectionMismatch {
                subject: "stale-mirror".to_string(),
                field: "definition",
            }
        ),
        other => panic!("expected a cron projection mismatch, got {other:?}"),
    }
    assert_untouched(&storage);

    let error = storage
        .commit_legacy_import_tampered(f.plan(), DEADLINE, ImportTamper::SentinelReadBack)
        .expect_err("a read-back aggregate that differs from the original refuses");
    match refusal(error) {
        ImportRefusal::ProjectionMismatch {
            domain: ImportDomain::Sentinel,
            mismatches,
            ..
        } => assert_eq!(
            mismatches[0],
            ProjectionMismatch {
                subject: "sentinel_root".to_string(),
                field: "runtime",
            }
        ),
        other => panic!("expected a Sentinel projection mismatch, got {other:?}"),
    }
    assert_untouched(&storage);

    let receipt = storage
        .commit_legacy_import(f.plan(), DEADLINE)
        .expect("the untampered plan commits");
    assert!(receipt.comparison.expect("comparison").is_clean());
    close(storage);
}

#[test]
fn storage_migration_crash_before_commit_rolls_back_and_restarts() {
    let f = Fixture::seeded();
    let originals = tree(&f.source());
    let storage = f.open();
    storage
        .inject_fault(Fault::FailNextCommit)
        .wait(DEADLINE)
        .expect("arm commit failure");

    let error = storage
        .commit_legacy_import(f.plan(), DEADLINE)
        .expect_err("the commit fails");

    assert!(
        matches!(
            error,
            MigrationError::Storage(StorageError::Injected { point: "commit" })
        ),
        "{error:?}"
    );
    assert_untouched(&storage);
    assert_eq!(tree(&f.source()), originals);
    let backup = tree(&f.backup().join("originals"));

    let receipt = storage
        .commit_legacy_import(f.plan(), DEADLINE)
        .expect("the restart commits");

    assert!(receipt.comparison.expect("comparison").is_clean());
    assert_eq!(
        cron_snapshot(&storage).states.len(),
        5,
        "four live jobs and one retired"
    );
    assert_eq!(sentinel_inventory(&storage).len(), 3);
    assert_eq!(
        tree(&f.backup().join("originals")),
        backup,
        "the backup is reused"
    );
    let replay = storage
        .commit_legacy_import(f.plan(), DEADLINE)
        .expect_err("a committed import never replays");
    assert!(matches!(
        refusal(replay),
        ImportRefusal::AlreadyImported { .. }
    ));
    assert_eq!(cron_snapshot(&storage).states.len(), 5);
    assert_eq!(tree(&f.source()), originals);
    close(storage);
}

#[test]
fn storage_migration_database_full_rolls_back_whole_import() {
    let f = Fixture::seeded();
    let storage = f.open();
    storage
        .inject_fault(Fault::DatabaseFull)
        .wait(DEADLINE)
        .expect("cap the database");

    let error = storage
        .commit_legacy_import(growing_plan(&f), DEADLINE)
        .expect_err("a full database refuses");

    assert!(
        matches!(error, MigrationError::Storage(StorageError::DatabaseFull)),
        "{error:?}"
    );
    assert_untouched(&storage);
    storage
        .inject_fault(Fault::ClearDatabaseFull)
        .wait(DEADLINE)
        .expect("lift the cap");
    let receipt = storage
        .commit_legacy_import(growing_plan(&f), DEADLINE)
        .expect("the retry commits");
    assert!(receipt.comparison.expect("comparison").is_clean());
    assert_eq!(sentinel_inventory(&storage).len(), 3);
    close(storage);
}

#[test]
fn storage_migration_after_commit_reopen_never_rereads_stale_json() {
    let f = Fixture::seeded();
    let storage = f.open();
    let receipt = import_seeded(&f, &storage);
    let snapshot = cron_snapshot(&storage);
    let inventory = sentinel_inventory(&storage);
    close(storage);
    f.put("crons.json", EMPTY_CRONS.as_bytes());
    f.put("sentinels.json", b"[]");

    let storage = f.open();
    let mut request = f.request();
    request.backup_dir = f.other("never-created");
    let outcome = storage
        .import_legacy_root(&request, &LegacyCodecs)
        .expect("a committed import answers");

    match outcome {
        ImportOutcome::AlreadyCommitted(found) => {
            assert_eq!(found.revision, receipt.revision);
            assert_eq!(found.manifest_sha256, receipt.manifest_sha256);
        }
        other => panic!("expected the committed marker, got {other:?}"),
    }
    assert!(
        !request.backup_dir.exists(),
        "no source was captured or read"
    );
    assert_eq!(cron_snapshot(&storage), snapshot);
    assert_eq!(sentinel_inventory(&storage), inventory);
    close(storage);
}

#[test]
fn storage_migration_corrupt_marker_refuses_instead_of_starting_empty() {
    let f = Fixture::seeded();
    let storage = f.open();
    import_seeded(&f, &storage);
    close(storage);
    let database = Storage::database_path(&f.instance());
    {
        let conn = rusqlite::Connection::open(&database).expect("open closed database");
        let removed = conn
            .execute(
                "DELETE FROM storage_meta WHERE key LIKE 'legacy_import.manifest_sha256:%'",
                [],
            )
            .expect("drop the digest row");
        assert_eq!(removed, 1);
    }

    let storage = f.open();

    let error = storage
        .legacy_import_marker()
        .wait(DEADLINE)
        .expect_err("an incomplete marker is corrupt");
    assert!(matches!(error, StorageError::Corrupt { .. }), "{error:?}");
    let error = storage
        .import_legacy_root(&f.request(), &LegacyCodecs)
        .expect_err("a corrupt marker never restarts the import");
    assert!(
        matches!(error, MigrationError::Storage(StorageError::Corrupt { .. })),
        "{error:?}"
    );
    assert_eq!(
        cron_snapshot(&storage).states.len(),
        5,
        "committed rows stay"
    );
    close(storage);
}

#[test]
fn storage_migration_occupied_domain_refuses() {
    let f = Fixture::seeded();
    let storage = f.open();
    storage
        .crons()
        .create(existing_job())
        .wait(DEADLINE)
        .expect("an existing cron row");

    let error = storage
        .import_legacy_root(&f.request(), &LegacyCodecs)
        .expect_err("an occupied domain refuses");

    assert_eq!(
        refusal(error),
        ImportRefusal::DomainOccupied {
            domain: ImportDomain::Cron
        }
    );
    assert_eq!(marker(&storage), None);
    let snapshot = cron_snapshot(&storage);
    assert_eq!(snapshot.states.len(), 1);
    assert_eq!(snapshot.states[0].job_id.as_str(), "existing");
    assert!(sentinel_inventory(&storage).is_empty());
    close(storage);
}

#[test]
fn storage_migration_tampered_backup_refuses_before_commit() {
    let f = Fixture::seeded();
    let storage = f.open();
    let plan = f.plan();
    let copy = plan.capture().original_path(&plan.capture().files()[0]);
    make_writable(&copy);
    fs::write(&copy, EMPTY_CRONS).expect("tamper with the backup copy");

    let error = storage
        .commit_legacy_import(plan, DEADLINE)
        .expect_err("a changed backup copy refuses");

    assert_eq!(
        refusal(error),
        ImportRefusal::BackupTampered {
            relative: "crons.json".to_string()
        }
    );
    assert_untouched(&storage);
    close(storage);
}

#[test]
fn storage_migration_tampered_capture_manifest_refuses_before_commit() {
    let f = Fixture::seeded();
    let storage = f.open();
    let plan = f.plan();
    let manifest = f.backup().join("capture.manifest");
    make_writable(&manifest);
    fs::write(&manifest, b"codeterm-legacy-capture 1\nend 0\n").expect("tamper");

    let error = storage
        .commit_legacy_import(plan, DEADLINE)
        .expect_err("a changed capture manifest refuses");

    assert_eq!(
        refusal(error),
        ImportRefusal::BackupTampered {
            relative: "capture.manifest".to_string()
        }
    );
    assert_untouched(&storage);
    close(storage);
}

#[test]
fn storage_migration_commit_reads_no_file_under_the_writer() {
    let f = Fixture::seeded();
    let storage = f.open();
    let plan = f.plan();
    let copy = plan.capture().original_path(&plan.capture().files()[0]);
    let hold = storage.hold_writer().expect("hold the writer");
    hold.wait_held(DEADLINE).expect("writer held");

    let receipt = storage
        .commit_legacy_import_after_submit(
            plan,
            DEADLINE,
            Box::new(move || {
                make_writable(&copy);
                fs::write(&copy, EMPTY_CRONS).expect("tamper after the pre-submit check");
                hold.release();
            }),
        )
        .expect("the queued transaction reads no file, so it commits");

    assert_eq!(
        receipt.archive,
        Err(ImportRefusal::BackupTampered {
            relative: "crons.json".to_string()
        })
    );
    assert_eq!(
        marker(&storage).map(|found| found.revision),
        Some(receipt.revision)
    );
    assert!(receipt.comparison.expect("comparison").is_clean());
    let recorded = lines(&fs::read(f.backup().join("import.receipt")).expect("receipt"));
    assert!(recorded
        .iter()
        .any(|words| words.starts_with(&line(&["archive", "backup-tampered"]))));
    close(storage);
}

#[test]
fn storage_migration_live_root_is_never_a_source() {
    let f = Fixture::new();
    let storage = f.open();
    fs::write(f.instance().join("crons.json"), CRONS).expect("legacy file in the live root");
    let request = ImportRequest::new(f.instance(), f.backup());

    let error = storage
        .import_legacy_root(&request, &LegacyCodecs)
        .expect_err("the live root is never imported");

    assert_eq!(refusal(error), ImportRefusal::LiveRoot);
    assert!(!f.backup().exists(), "nothing was captured");
    assert_untouched(&storage);
    close(storage);
}
