//! storage_migration_ conservation: exact identities, nulls, order, history and Sentinel
//! state survive one commit; both copies of a divergent last run are kept and recorded;
//! the import causes no action; the manifest and receipt formats are stable.

use super::*;
use std::collections::HashSet;

/// Keys the typed cron decoder does not keep: in the envelope, a job, its last run and a
/// retired job's history run.
const ANNOTATED_CRONS: &str = r#"{
  "version": "1", "schema_hint": 2,
  "jobs": [
    {"id": "noted", "name": "noted", "workspace_id": "ws", "schedule": "0 * * * *",
     "enabled": true, "agent": "claude", "task": "t", "created_at": 1, "legacy_color": "red",
     "last_run": {"run_id": "n1", "started_at": 1, "finished_at": 2, "status": "Success",
       "tab_id": null, "exit_hint": 3}}
  ],
  "history": {
    "noted": [{"run_id": "n1", "started_at": 1, "finished_at": 2, "status": "Success",
      "tab_id": null}],
    "gone": [{"run_id": "g1", "started_at": 1, "finished_at": 2, "status": "Success",
      "tab_id": null, "worker": "w1"}]
  }
}"#;

fn fields(list: &[CronRunField]) -> String {
    list.iter()
        .map(|field| field.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

fn unconfirmed(transitions: &[TransitionRecord]) -> usize {
    transitions
        .iter()
        .flat_map(|transition| &transition.handlers)
        .filter(|result| result.status == HandlerStatus::Unconfirmed)
        .count()
}

/// Durable evidence of any action the import could have caused.
#[derive(Debug, Default, PartialEq, Eq)]
struct EffectLedger {
    cron_runs_started: usize,
    cron_schedules_moved: usize,
    sentinel_transitions_added: usize,
    sentinel_leases_moved: usize,
    sentinel_claims_settled: usize,
    files_outside_storage: usize,
}

#[test]
fn storage_migration_one_transaction_conserves_cron_and_sentinel_exactly() {
    let f = Fixture::seeded();
    let storage = f.open();
    let before = storage.metrics().committed_operations;

    let receipt = import_seeded(&f, &storage);

    assert_eq!(
        storage.metrics().committed_operations - before,
        1,
        "one transaction"
    );
    let found = marker(&storage).expect("completion marker");
    assert_eq!(
        (found.revision, found.manifest_sha256, found.format),
        (
            receipt.revision,
            receipt.manifest_sha256,
            IMPORT_MANIFEST_FORMAT
        )
    );

    let expected = decoded_cron();
    let snapshot = cron_snapshot(&storage);
    let live: Vec<&StoredCronJob> = snapshot
        .states
        .iter()
        .filter_map(|state| state.job.as_ref())
        .collect();
    assert_eq!(live.len(), expected.jobs.len());
    for (stored, original) in live.iter().zip(&expected.jobs) {
        let id = original.record.id.as_str();
        assert_eq!(stored.record, original.record, "{id}");
        assert_eq!(stored.last_run, original.last_run, "{id}");
    }
    let history = |id: &str| -> Vec<CronRunRecord> {
        snapshot
            .states
            .iter()
            .find(|state| state.job_id.as_str() == id)
            .map(|state| state.history.clone())
            .expect("identity")
    };
    for listed in &expected.histories {
        assert_eq!(
            history(listed.job_id.as_str()),
            listed.runs,
            "{}",
            listed.job_id.as_str()
        );
    }
    let retired = snapshot
        .states
        .iter()
        .find(|state| state.job_id.as_str() == "deleted-job")
        .expect("retired identity");
    assert!(retired.job.is_none() && retired.identity.retired);
    assert!(
        history("unlisted").is_empty(),
        "an unlisted last run stays outside history"
    );

    let records = decoded_sentinels();
    let inventory = sentinel_inventory(&storage);
    assert_eq!(inventory.len(), records.len());
    for (stored, original) in inventory.iter().zip(&records) {
        let record = &stored.record;
        assert_eq!(record.identity, original.identity);
        assert_eq!(record.scenario_id, original.scenario_id);
        assert_eq!(
            (record.definition.version, record.definition.provenance),
            (1, DefinitionProvenance::Imported)
        );
        assert_eq!(record.definition.definition, original.definition);
        assert_eq!(record.runtime, original.runtime);
        assert_eq!(record.transitions, original.transitions);
    }

    let comparison = receipt.comparison.as_ref().expect("comparison");
    assert!(comparison.is_clean());
    let counts: Vec<(ImportDomain, u64, u64)> = comparison
        .domains
        .iter()
        .map(|domain| {
            (
                domain.domain,
                domain.committed_entities,
                domain.committed_items,
            )
        })
        .collect();
    assert_eq!(
        counts,
        [
            (ImportDomain::Cron, 4, 4),
            (ImportDomain::Sentinel, 3, 3),
            (ImportDomain::Desk, 0, 0)
        ]
    );
    assert_eq!(receipt.archive, Ok(()));
    assert!(matches!(receipt.recorded, RecordedManifest::File(_)));
    close(storage);
}

#[test]
fn storage_migration_divergent_pairs_are_preserved_and_recorded() {
    let f = Fixture::seeded();
    let storage = f.open();

    let receipt = import_seeded(&f, &storage);

    let snapshot = cron_snapshot(&storage);
    let state = |id: &str| {
        snapshot
            .states
            .iter()
            .find(|state| state.job_id.as_str() == id)
            .expect("identity")
    };
    let stale = state("stale-mirror");
    let job = stale.job.as_ref().expect("live job");
    let copy = job.last_run.as_ref().expect("own last run");
    let row = &stale.history[1];
    assert_eq!((copy.run_id.as_str(), row.run_id.as_str()), ("r1", "r1"));
    assert_eq!(
        (copy.status, row.status),
        (CronRunState::Running, CronRunState::Cancelled)
    );
    assert_eq!((copy.finished_at, row.finished_at), (None, Some(15)));
    let divergence = job.divergence.as_ref().expect("divergence provenance");
    assert_eq!(divergence.origin, CronDivergenceOrigin::LegacyImport);
    assert_eq!(divergence.imported_revision, receipt.revision);
    assert_eq!(
        divergence.imported_differing,
        [
            CronRunField::FinishedAt,
            CronRunField::Status,
            CronRunField::TabId,
            CronRunField::Summary
        ]
    );
    let drift = state("summary-drift")
        .job
        .as_ref()
        .and_then(|job| job.divergence.as_ref())
        .expect("summary divergence");
    assert_eq!(drift.imported_differing, [CronRunField::Summary]);
    let unlisted = state("unlisted").job.as_ref().expect("live job");
    assert!(unlisted.divergence.is_none());

    let pairs: Vec<(&str, &str, &[CronRunField])> = receipt
        .divergences
        .iter()
        .map(|found| {
            (
                found.job_id.as_str(),
                found.run_id.as_str(),
                found.imported_differing.as_slice(),
            )
        })
        .collect();
    assert_eq!(
        pairs,
        [
            (
                "stale-mirror",
                "r1",
                &[
                    CronRunField::FinishedAt,
                    CronRunField::Status,
                    CronRunField::TabId,
                    CronRunField::Summary
                ][..]
            ),
            ("summary-drift", "s1", &[CronRunField::Summary][..]),
        ]
    );
    let manifest = lines(&fs::read(f.backup().join("import.manifest")).expect("manifest"));
    let recorded: Vec<&Vec<String>> = manifest
        .iter()
        .filter(|words| words[0] == "divergence")
        .collect();
    assert_eq!(recorded.len(), receipt.divergences.len());
    for (words, found) in recorded.iter().zip(&receipt.divergences) {
        assert_ne!(found.history_copy_sha256, found.last_run_copy_sha256);
        let differing = fields(&found.imported_differing);
        let expected = line(&[
            "divergence",
            found.job_id.as_str(),
            found.run_id.as_str(),
            "legacy_import",
            "at-marker",
            &differing,
            &found.history_copy_sha256.to_hex(),
            &found.last_run_copy_sha256.to_hex(),
        ]);
        assert_eq!(**words, expected);
    }
    let receipt_lines = lines(&fs::read(f.backup().join("import.receipt")).expect("receipt"));
    let revision = receipt.revision.to_string();
    assert!(receipt_lines.contains(&line(&["revision", &revision])));
    let bound = line(&[
        "divergence",
        "stale-mirror",
        "r1",
        "legacy_import",
        &revision,
    ]);
    assert!(receipt_lines.iter().any(|words| words.starts_with(&bound)));
    close(storage);
}

#[test]
fn storage_migration_import_constructs_no_effects() {
    let f = Fixture::seeded();
    let originals = tree(&f.source());
    let source_cron = decoded_cron();
    let source_sentinels = decoded_sentinels();
    assert!(source_cron
        .jobs
        .iter()
        .any(|job| job.record.enabled && job.record.next_run.is_some_and(|at| at < 100)));
    assert!(source_sentinels
        .iter()
        .any(|record| !record.runtime.paused && !record.runtime.stopped));
    let storage = f.open();
    let before = storage.metrics().committed_operations;

    import_seeded(&f, &storage);

    let snapshot = cron_snapshot(&storage);
    let inventory = sentinel_inventory(&storage);
    let mut known: HashSet<(String, String)> = HashSet::new();
    for history in &source_cron.histories {
        for run in &history.runs {
            known.insert((
                history.job_id.as_str().to_string(),
                run.run_id.as_str().to_string(),
            ));
        }
    }
    for job in &source_cron.jobs {
        if let Some(run) = &job.last_run {
            known.insert((
                job.record.id.as_str().to_string(),
                run.run_id.as_str().to_string(),
            ));
        }
    }
    let mut ledger = EffectLedger::default();
    for state in &snapshot.states {
        let last = state.job.as_ref().and_then(|job| job.last_run.as_ref());
        for run in state.history.iter().chain(last) {
            let key = (
                state.job_id.as_str().to_string(),
                run.run_id.as_str().to_string(),
            );
            ledger.cron_runs_started += usize::from(!known.contains(&key));
        }
    }
    let live = snapshot
        .states
        .iter()
        .filter_map(|state| state.job.as_ref());
    for (stored, original) in live.zip(&source_cron.jobs) {
        let (now, then) = (&stored.record, &original.record);
        let moved = (
            now.next_run,
            now.enabled,
            now.consecutive_failures,
            now.jitter_max_ms,
        ) != (
            then.next_run,
            then.enabled,
            then.consecutive_failures,
            then.jitter_max_ms,
        );
        ledger.cron_schedules_moved += usize::from(moved);
    }
    for (stored, original) in inventory.iter().zip(&source_sentinels) {
        let (now, then) = (&stored.record, original);
        ledger.sentinel_transitions_added +=
            now.transitions.len().saturating_sub(then.transitions.len());
        let lease = |runtime: &StageRuntime| {
            (
                runtime.generation,
                runtime.next_run,
                runtime.paused,
                runtime.stopped,
            )
        };
        ledger.sentinel_leases_moved += usize::from(lease(&now.runtime) != lease(&then.runtime));
        ledger.sentinel_claims_settled +=
            unconfirmed(&then.transitions).saturating_sub(unconfirmed(&now.transitions));
    }
    let instance: Vec<String> = fs::read_dir(f.instance())
        .expect("instance root")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    ledger.files_outside_storage = instance.iter().filter(|name| *name != "storage").count();
    ledger.files_outside_storage += usize::from(tree(&f.source()) != originals);

    assert_eq!(ledger, EffectLedger::default());
    assert_eq!(
        storage.metrics().committed_operations - before,
        1,
        "the import is the only write"
    );
    let open_claims: usize = inventory
        .iter()
        .map(|stored| unconfirmed(&stored.record.transitions))
        .sum();
    assert_eq!(open_claims, 2, "unconfirmed claims stay unconfirmed");
    close(storage);
}

#[test]
fn storage_migration_unknown_cron_keys_are_recorded_not_dropped() {
    let f = Fixture::new();
    f.put("crons.json", ANNOTATED_CRONS.as_bytes());
    f.put("sentinels.json", SENTINELS.as_bytes());
    let storage = f.open();

    let receipt = import_seeded(&f, &storage);

    let manifest = lines(&fs::read(f.backup().join("import.manifest")).expect("manifest"));
    let recorded: Vec<&Vec<String>> = manifest
        .iter()
        .filter(|words| words.len() > 2 && words[0] == "rule" && words[2] == "unknown-keys")
        .collect();
    assert_eq!(
        recorded,
        [
            &line(&["rule", "cron", "unknown-keys", "~", "schema_hint"]),
            &line(&["rule", "cron", "unknown-keys", "gone", "history.g1.worker"]),
            &line(&[
                "rule",
                "cron",
                "unknown-keys",
                "noted",
                "last_run.exit_hint,legacy_color"
            ]),
        ]
    );
    let copy = fs::read(f.backup().join("originals").join("crons.json")).expect("backup copy");
    assert_eq!(
        copy,
        ANNOTATED_CRONS.as_bytes(),
        "the original keeps every key"
    );
    let snapshot = cron_snapshot(&storage);
    let noted = snapshot
        .states
        .iter()
        .find(|state| state.job_id.as_str() == "noted")
        .and_then(|state| state.job.as_ref())
        .expect("live job");
    assert!(
        noted.divergence.is_none(),
        "an ignored key is no divergence"
    );
    assert!(receipt.comparison.expect("comparison").is_clean());
    close(storage);
}

#[test]
fn storage_migration_manifests_are_versioned_deterministic_and_parseable() {
    let f = Fixture::seeded();
    let first = f.plan();
    let second = f.plan();
    assert_eq!(first.manifest_digest(), second.manifest_digest());
    let published = fs::read(f.backup().join("import.manifest")).expect("import manifest");
    assert_eq!(published, first.manifest_text());
    assert_eq!(Sha256Digest::of(&published), first.manifest_digest());

    let manifest = lines(&published);
    let format = IMPORT_MANIFEST_FORMAT.to_string();
    let capture = first.capture().digest().to_hex();
    assert_eq!(manifest[0], line(&["codeterm-legacy-import", &format]));
    assert_eq!(manifest[1], line(&["capture", &capture]));
    assert_eq!(manifest.last(), Some(&line(&["end"])));
    let (jobs, history, batch) = (
        MAX_CRON_JOBS.to_string(),
        MAX_CRON_HISTORY.to_string(),
        MAX_CRON_BATCH.to_string(),
    );
    let (stages, import_batch) = (
        MAX_SENTINEL_RECORDS.to_string(),
        MAX_SENTINEL_BATCH.to_string(),
    );
    for expected in [
        line(&["provenance", "cron", "current", "crons.json"]),
        line(&["provenance", "blueprint", "current", "sentinels.json"]),
        line(&["status", "cron", "crons.json", "valid"]),
        line(&["status", "blueprint", "sentinels.json", "valid"]),
        line(&["bound", "cron", "jobs", &jobs, "4"]),
        line(&["bound", "cron", "history-per-job", &history, "2"]),
        line(&["bound", "cron", "max-history", &history, "100"]),
        line(&["bound", "cron", "write-batch", &batch, "0"]),
        line(&["rule", "cron", "fields", "repository-validated"]),
        line(&[
            "rule",
            "cron",
            "write-batch",
            "typed-cronwrite-batches-only",
        ]),
        line(&["bound", "blueprint", "stages", &stages, "3"]),
        line(&["bound", "blueprint", "import-batch", &import_batch, "3"]),
        line(&["rule", "blueprint", "fields", "repository-validated"]),
        line(&["count", "cron", "retired-identities", "1"]),
        line(&["count", "cron", "empty-live-histories", "1"]),
        line(&["count", "cron", "unlisted-last-runs", "1"]),
        line(&["count", "cron", "divergent-last-runs", "2"]),
        line(&["count", "blueprint", "transitions", "3"]),
        line(&["count", "blueprint", "handler-results", "5"]),
        line(&["count", "blueprint", "unconfirmed-claims", "2"]),
        line(&["count", "blueprint", "cron-targets-absent", "1"]),
    ] {
        assert!(manifest.contains(&expected), "missing {expected:?}");
    }

    let storage = f.open();
    let receipt = storage
        .commit_legacy_import(first, DEADLINE)
        .expect("commit");

    let recorded = lines(&fs::read(f.backup().join("import.receipt")).expect("receipt"));
    let digest = receipt.manifest_sha256.to_hex();
    let revision = receipt.revision.to_string();
    assert_eq!(recorded[0], line(&["codeterm-legacy-receipt", &format]));
    for expected in [
        line(&["manifest", &digest]),
        line(&["capture", &capture]),
        line(&["revision", &revision]),
        line(&["archive", "verified"]),
        line(&[
            "compare",
            "cron",
            "entities",
            "4",
            "4",
            "items",
            "4",
            "4",
            "mismatches",
            "0",
            "elided",
            "0",
        ]),
        line(&[
            "compare",
            "blueprint",
            "entities",
            "3",
            "3",
            "items",
            "3",
            "3",
            "mismatches",
            "0",
            "elided",
            "0",
        ]),
    ] {
        assert!(recorded.contains(&expected), "missing {expected:?}");
    }
    assert_eq!(recorded.last(), Some(&line(&["end"])));
    close(storage);
}
