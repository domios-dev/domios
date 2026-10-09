//! storage_migration_ selection: first-install absence differs from absent, malformed,
//! partial, unsupported and unreadable originals, and recovery is only ever explicit.

use super::*;

const BAK_VALID: &str = "state-backups/crons.json/0000000000001-000001.bak";
const BAK_PARTIAL: &str = "state-backups/crons.json/0000000000002-000001.bak";
const SENTINEL_BAK: &str = "state-backups/sentinels.json/0000000000001-000001.bak";

/// Well-formed JSON whose stopped solo stage appears twice: a semantic conflict.
fn conflicting_sentinels() -> String {
    let mut stages: Vec<serde_json::Value> = serde_json::from_str(SENTINELS).expect("fixture");
    let duplicate = stages.last().cloned().expect("solo stage");
    stages.push(duplicate);
    serde_json::to_string(&stages).expect("conflicting inventory")
}

fn starts(words: &[String], prefix: &[&str]) -> bool {
    words.starts_with(&line(prefix))
}

fn import_manifest(f: &Fixture) -> Vec<Vec<String>> {
    lines(&fs::read(f.backup().join("import.manifest")).expect("import manifest"))
}

/// The refusal one damaged current cron original produces, with nothing committed.
fn refusal_for(current: &[u8]) -> ImportRefusal {
    let f = Fixture::seeded();
    f.put("crons.json", current);
    let storage = f.open();
    let error = storage
        .import_legacy_root(&f.request(), &LegacyCodecs)
        .expect_err("a damaged current original refuses");
    assert_untouched(&storage);
    close(storage);
    refusal(error)
}

#[test]
fn storage_migration_first_install_absence_commits_empty_domains() {
    let f = Fixture::new();
    let storage = f.open();

    let receipt = committed(storage.import_legacy_root(&f.request(), &LegacyCodecs));

    let manifest = import_manifest(&f);
    assert!(manifest.contains(&line(&["provenance", "cron", "first-install"])));
    assert!(manifest.contains(&line(&["provenance", "blueprint", "first-install"])));
    assert!(manifest.contains(&line(&["provenance", "desk", "first-install"])));
    assert_eq!(
        marker(&storage).map(|found| found.revision),
        Some(receipt.revision)
    );
    assert!(cron_snapshot(&storage).states.is_empty());
    assert!(sentinel_inventory(&storage).is_empty());
    assert!(storage
        .desk()
        .snapshot()
        .wait(DEADLINE)
        .unwrap()
        .value
        .snapshot
        .workspaces
        .is_empty());
    assert!(receipt.comparison.expect("comparison").is_clean());
    close(storage);
}

#[test]
fn storage_migration_absent_current_with_recovery_assets_requires_selection() {
    let f = Fixture::new();
    f.put("sentinels.json", SENTINELS.as_bytes());
    f.put(BAK_VALID, CRONS.as_bytes());
    let storage = f.open();

    let error = storage
        .import_legacy_root(&f.request(), &LegacyCodecs)
        .expect_err("absence beside recovery assets is not a first install");

    let MigrationError::Refused(report) = &error else {
        panic!("expected a refusal, got {error:?}");
    };
    assert_eq!(
        report.refusal,
        ImportRefusal::RecoveryRequired {
            domain: ImportDomain::Cron,
            current: SourceStatus::Absent,
            candidates: vec![RecoveryCandidate {
                relative: BAK_VALID.to_string(),
                role: SourceRole::RotatingBackup,
                status: SourceStatus::Valid,
            }],
        }
    );
    let RecordedManifest::File(path) = &report.recorded else {
        panic!("the refusal is recorded");
    };
    let recorded = lines(&fs::read(path).expect("refusal manifest"));
    assert!(recorded.contains(&line(&[
        "candidate",
        "cron",
        BAK_VALID,
        "rotating-backup",
        "valid"
    ])));
    assert_untouched(&storage);

    let acknowledged = f.select(ImportDomain::Cron, SourceSelection::AcknowledgeAbsent);
    let receipt = committed(storage.import_legacy_root(&acknowledged, &LegacyCodecs));

    assert!(import_manifest(&f).contains(&line(&["provenance", "cron", "acknowledged-absent"])));
    assert!(
        cron_snapshot(&storage).states.is_empty(),
        "the backup is retained, not imported"
    );
    assert_eq!(sentinel_inventory(&storage).len(), 3);
    assert!(receipt.comparison.expect("comparison").is_clean());
    close(storage);
}

#[test]
fn storage_migration_malformed_current_requires_explicit_backup_selection() {
    let f = Fixture::seeded();
    let malformed = b"{\"version\": \"1\", \"jobs\": [}";
    f.put("crons.json", malformed);
    f.put(BAK_VALID, CRONS.as_bytes());
    f.put(BAK_PARTIAL, b"{\"version\": \"1\", \"jobs\": [");
    let originals = tree(&f.source());
    let storage = f.open();

    let error = storage
        .import_legacy_root(&f.request(), &LegacyCodecs)
        .expect_err("a malformed current original never imports silently");

    match refusal(error) {
        ImportRefusal::RecoveryRequired {
            domain: ImportDomain::Cron,
            current: SourceStatus::Malformed(_),
            candidates,
        } => {
            let found: Vec<(&str, &str)> = candidates
                .iter()
                .map(|candidate| (candidate.relative.as_str(), candidate.status.code()))
                .collect();
            assert_eq!(found, [(BAK_VALID, "valid"), (BAK_PARTIAL, "partial")]);
        }
        other => panic!("expected a recovery requirement, got {other:?}"),
    }
    assert_untouched(&storage);

    let selected = f.select(
        ImportDomain::Cron,
        SourceSelection::Backup(BAK_VALID.to_string()),
    );
    let receipt = committed(storage.import_legacy_root(&selected, &LegacyCodecs));

    let manifest = import_manifest(&f);
    assert!(manifest.contains(&line(&["provenance", "cron", "backup", BAK_VALID])));
    assert!(manifest
        .iter()
        .any(|words| starts(words, &["status", "cron", "crons.json", "malformed"])));
    let snapshot = cron_snapshot(&storage);
    let live: Vec<&str> = snapshot
        .states
        .iter()
        .filter(|state| state.job.is_some())
        .map(|state| state.job_id.as_str())
        .collect();
    assert_eq!(live, ["stale-mirror", "summary-drift", "unlisted", "quiet"]);
    let retained = fs::read(f.backup().join("originals").join("crons.json"));
    assert_eq!(
        retained.expect("damaged original captured"),
        malformed.to_vec()
    );
    assert_eq!(tree(&f.source()), originals);
    assert!(receipt.comparison.expect("comparison").is_clean());
    close(storage);
}

#[test]
fn storage_migration_partial_unsupported_and_malformed_are_distinct() {
    assert!(matches!(
        refusal_for(b""),
        ImportRefusal::RecoveryRequired {
            current: SourceStatus::Partial(_),
            ..
        }
    ));
    assert!(matches!(
        refusal_for(b"{\"version\": \"1\", \"jobs\": [{\"id\""),
        ImportRefusal::RecoveryRequired {
            current: SourceStatus::Partial(_),
            ..
        }
    ));
    assert!(matches!(
        refusal_for(b"not json"),
        ImportRefusal::RecoveryRequired {
            current: SourceStatus::Malformed(_),
            ..
        }
    ));
    assert!(matches!(
        refusal_for(b"\xff\xfe"),
        ImportRefusal::RecoveryRequired {
            current: SourceStatus::Malformed(_),
            ..
        }
    ));
    assert!(matches!(
        refusal_for(br#"{"version": "2", "jobs": [], "history": {}}"#),
        ImportRefusal::Decode {
            domain: ImportDomain::Cron,
            status: SourceStatus::Unsupported(_),
            ..
        }
    ));
}

#[test]
fn storage_migration_sentinel_conflict_is_not_recoverable() {
    let f = Fixture::seeded();
    f.put("sentinels.json", conflicting_sentinels().as_bytes());
    f.put(SENTINEL_BAK, SENTINELS.as_bytes());
    let storage = f.open();

    let error = storage
        .import_legacy_root(&f.request(), &LegacyCodecs)
        .expect_err("a conflicting current original refuses");

    assert!(matches!(
        refusal(error),
        ImportRefusal::Decode {
            domain: ImportDomain::Sentinel,
            status: SourceStatus::Conflict(_),
            ..
        }
    ));
    let selected = f.select(
        ImportDomain::Sentinel,
        SourceSelection::Backup(SENTINEL_BAK.to_string()),
    );
    let error = storage
        .import_legacy_root(&selected, &LegacyCodecs)
        .expect_err("a backup never replaces a conflicting current original");
    assert!(matches!(
        refusal(error),
        ImportRefusal::InvalidSelection {
            domain: ImportDomain::Sentinel,
            ..
        }
    ));
    assert_untouched(&storage);
    close(storage);
}

#[test]
fn storage_migration_unreadable_current_is_refused_as_unreadable() {
    let f = Fixture::new();
    fs::create_dir_all(f.source().join("crons.json")).expect("a directory where the file was");
    f.put("sentinels.json", SENTINELS.as_bytes());
    let storage = f.open();

    let error = storage
        .import_legacy_root(&f.request(), &LegacyCodecs)
        .expect_err("an unreadable current original refuses");

    assert!(matches!(
        refusal(error),
        ImportRefusal::RecoveryRequired {
            domain: ImportDomain::Cron,
            current: SourceStatus::Unreadable(_),
            ..
        }
    ));
    assert_untouched(&storage);
    assert!(
        f.source().join("crons.json").is_dir(),
        "the original stays in place"
    );
    let capture = lines(&fs::read(f.backup().join("capture.manifest")).expect("capture"));
    assert!(capture.iter().any(|words| starts(
        words,
        &[
            "file",
            "cron",
            "current",
            "crons.json",
            "unreadable",
            "directory"
        ]
    )));
    close(storage);
}
