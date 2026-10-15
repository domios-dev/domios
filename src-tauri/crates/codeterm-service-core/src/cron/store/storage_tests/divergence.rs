//! storage_cron_ last-run divergence: an original last run that differs from its own
//! history row survives import and reopen as its own copy, and evolves like the JSON file.

use std::path::PathBuf;

use super::*;
use crate::cron::store::{
    commit, finish_run, read_legacy_cron_file, CronCommitError, CronRunStatus, CronScope,
    CronStore, RunSession,
};
use crate::storage::{
    CronDivergenceOrigin, CronJobId, CronRunField, CronRunId, CronRunRecord, CronRunState,
    CronWrite, Storage, StorageConfig, StorageError,
};

const STALE: &str = "stale-mirror";
const DRIFTED: &str = "summary-drift";

/// Two divergent pairs: a last run still Running after its history row was cancelled,
/// and a last run missing only the summary its history row carries.
const DIVERGENT_FILE: &str = r#"{
  "version": "1",
  "jobs": [
    {"id": "stale-mirror", "name": "stale mirror", "workspace_id": "ws-stale",
     "schedule": "0 * * * *", "enabled": true, "agent": "claude", "task": "t",
     "created_at": 1, "max_history": 5, "jitter_max_ms": 0,
     "last_run": {"run_id": "r1", "started_at": 10, "finished_at": null,
       "status": "Running", "tab_id": "tab-r1", "session_id": null, "summary": null}},
    {"id": "summary-drift", "name": "summary drift", "workspace_id": "ws-drift",
     "schedule": "0 * * * *", "enabled": true, "agent": "claude", "task": "t",
     "created_at": 2, "jitter_max_ms": 0,
     "last_run": {"run_id": "s1", "started_at": 20, "finished_at": 21,
       "status": "Success", "tab_id": null}}
  ],
  "history": {
    "stale-mirror": [
      {"run_id": "r0", "started_at": 5, "finished_at": 6, "status": "Success", "tab_id": null},
      {"run_id": "r1", "started_at": 10, "finished_at": 15, "status": "Cancelled",
       "tab_id": null, "summary": "cancelled by owner"}
    ],
    "summary-drift": [
      {"run_id": "s1", "started_at": 20, "finished_at": 21, "status": "Success",
       "tab_id": null, "summary": "done"}
    ]
  }
}"#;

/// Imports the original into the fixture and returns the original's path.
fn import_divergent(f: &Fixture) -> PathBuf {
    let originals = f.temp.path().join("originals");
    std::fs::create_dir_all(&originals).expect("originals dir");
    let path = originals.join("crons.json");
    std::fs::write(&path, DIVERGENT_FILE).expect("write original");
    let import = read_legacy_cron_file(&path)
        .expect("a divergent pair is no conflict")
        .expect("present");
    f.storage
        .crons()
        .import(import)
        .wait(DEADLINE)
        .expect("import keeps both copies");
    *f.cron.lock() = f.rehydrated();
    path
}

fn divergence_of(f: &Fixture, job_id: &str) -> Option<crate::storage::CronLastRunDivergence> {
    let snapshot = f.snapshot();
    let state = state_of(&snapshot, job_id);
    state.job.as_ref().expect("live job").divergence.clone()
}

fn assert_same_as(legacy: &CronStore, f: &Fixture) {
    let cache = f.cron.lock();
    assert_eq!(legacy.list(), cache.list(), "definitions and last runs");
    assert_eq!(histories(legacy), histories(&cache), "history rows");
    drop(cache);
    f.assert_cache_is_committed();
}

#[test]
fn storage_cron_divergent_last_run_survives_import_and_reopen() {
    let f = fixture();
    let path = import_divergent(&f);
    let legacy = CronStore::new_with_path_and_clock(path, f.clock.clone());
    assert_same_as(&legacy, &f);

    let snapshot = f.snapshot();
    let stale = state_of(&snapshot, STALE);
    let job = stale.job.as_ref().expect("live job");
    let copy = job.last_run.as_ref().expect("own last run");
    let row = &stale.history[1];
    assert_eq!((copy.run_id.as_str(), row.run_id.as_str()), ("r1", "r1"));
    assert_eq!((copy.finished_at, row.finished_at), (None, Some(15)));
    assert_eq!(
        (copy.tab_id.as_deref(), row.tab_id.as_deref()),
        (Some("tab-r1"), None)
    );
    assert_eq!(
        (copy.summary.as_deref(), row.summary.as_deref()),
        (None, Some("cancelled by owner"))
    );
    let divergence = job.divergence.as_ref().expect("provenance");
    assert_eq!(divergence.origin, CronDivergenceOrigin::LegacyImport);
    assert_eq!(
        divergence.imported_differing,
        [
            CronRunField::FinishedAt,
            CronRunField::Status,
            CronRunField::TabId,
            CronRunField::Summary
        ]
    );
    let drifted = divergence_of(&f, DRIFTED).expect("summary-only divergence");
    assert_eq!(drifted.imported_differing, [CronRunField::Summary]);

    f.storage.shutdown().wait(DEADLINE).expect("shutdown");
    let reopened = Storage::open(instance(&f.temp), StorageConfig::default()).expect("reopen");
    let fresh = hydrate(&reopened, &f.temp, &f.clock);
    assert_eq!(fresh.list(), legacy.list(), "both copies survive a reopen");
    assert_eq!(histories(&fresh), histories(&legacy));
    reopened
        .shutdown()
        .wait(DEADLINE)
        .expect("shutdown reopened");
}

#[test]
fn storage_cron_divergent_last_run_evolves_like_the_legacy_file() {
    let f = fixture();
    let path = import_divergent(&f);
    let mut legacy = CronStore::new_with_path_and_clock(path, f.clock.clone());
    let imported = divergence_of(&f, STALE).expect("imported divergence");

    legacy.update(STALE, rename("renamed")).unwrap();
    let patched = rows(
        commit(&f.cron, |store| {
            store.prepare_update(STALE, rename("renamed"))
        })
        .unwrap(),
    );
    assert_same_as(&legacy, &f);
    assert_eq!(
        patched, 2,
        "a patch writes the definition and identity only"
    );
    assert_eq!(
        divergence_of(&f, STALE),
        Some(imported.clone()),
        "a patch keeps it"
    );

    let session = RunSession {
        session_id: "late-session",
        cwd: Some("/late"),
    };
    legacy.set_run_session(STALE, "r1", "late-session", Some("/late"));
    let bound = rows(
        commit(&f.cron, |store| {
            Ok(store.prepare_set_run_session(STALE, "r1", session))
        })
        .unwrap(),
    );
    assert_same_as(&legacy, &f);
    assert_eq!(bound, 3, "history row, divergent copy and identity");

    for store in [&mut legacy, &mut *f.cron.lock()] {
        store.register_tab("tab-r1", STALE, "r1");
    }
    legacy
        .set_tab_awaiting_owner_prompt("tab-r1", true)
        .unwrap();
    let prompted = commit(&f.cron, |store| {
        Ok(store.prepare_owner_prompt("tab-r1", true))
    })
    .unwrap();
    assert!(prompted.value, "only the Running copy moves to awaiting");
    assert_eq!(rows(prompted), 2, "divergent copy and identity");
    assert_same_as(&legacy, &f);

    assert!(!legacy
        .update_run_status(STALE, "r1", CronRunStatus::Failed)
        .unwrap());
    assert!(
        !finish_run(&f.cron, STALE, "r1", CronRunStatus::Failed, None).unwrap(),
        "the history row was already terminal, as in the legacy file"
    );
    assert_same_as(&legacy, &f);
    assert_eq!(
        divergence_of(&f, STALE),
        Some(imported),
        "the copies still differ; import provenance is never recomputed"
    );

    legacy.record_run(STALE, running("r2")).unwrap();
    let started = rows(
        commit(&f.cron, |store| {
            store.prepare_record_run(STALE, running("r2"))
        })
        .unwrap(),
    );
    assert_same_as(&legacy, &f);
    assert_eq!(
        started, 4,
        "run insert, copy removal, definition and identity"
    );
    assert_eq!(
        divergence_of(&f, STALE),
        None,
        "the next run replaces the copy"
    );

    legacy
        .set_run_report(DRIFTED, "s1", "done", "delivered".to_string())
        .unwrap();
    let converged = rows(
        commit(&f.cron, |store| {
            Ok(store.prepare_set_run_report(DRIFTED, "s1", "done", "delivered"))
        })
        .unwrap(),
    );
    assert_same_as(&legacy, &f);
    assert_eq!(
        converged, 4,
        "history row, copy update, copy removal and identity"
    );
    assert_eq!(
        divergence_of(&f, DRIFTED),
        None,
        "equal copies are one row again"
    );
}

#[test]
fn storage_cron_divergence_is_never_manufactured_and_leaves_with_its_definition() {
    let f = fixture();
    import_divergent(&f);
    let live = f.create(params("consistent"));
    f.start(&live.id, "c1");
    let before = f.snapshot();
    let identity = |job_id: &str| state_of(&before, job_id).identity.revision;
    let copy = |run_id: &str| CronRunRecord {
        run_id: CronRunId::new(run_id).unwrap(),
        started_at: 1,
        finished_at: None,
        status: CronRunState::Running,
        tab_id: Some("manufactured".into()),
        session_id: None,
        cwd: None,
        summary: None,
        report_delivery: None,
    };
    let write = |job_id: &str, run_id: &str| CronWrite::UpdateRuns {
        job_id: CronJobId::new(job_id).unwrap(),
        expect: identity(job_id),
        runs: Vec::new(),
        job: None,
        last_run: Some(copy(run_id)),
    };
    for (job_id, run_id) in [(live.id.as_str(), "c1"), (STALE, "r0")] {
        let refused = f
            .storage
            .crons()
            .apply(vec![write(job_id, run_id)])
            .wait(DEADLINE);
        assert!(
            matches!(refused, Err(StorageError::Invalid { .. })),
            "{job_id}/{run_id}: {refused:?}"
        );
    }
    assert_eq!(f.snapshot(), before, "a refused write changes nothing");

    let session = RunSession {
        session_id: "s",
        cwd: None,
    };
    commit(&f.cron, |store| {
        Ok(store.prepare_set_run_session(&live.id, "c1", session))
    })
    .unwrap();
    assert_eq!(
        divergence_of(&f, &live.id),
        None,
        "live updates keep one copy"
    );

    let retired = commit(&f.cron, |store| store.prepare_delete(STALE, true)).unwrap();
    assert_eq!(rows(retired), 3, "copy, definition and identity");
    let snapshot = f.snapshot();
    let stale = state_of(&snapshot, STALE);
    assert!(stale.identity.retired && stale.job.is_none());
    assert_eq!(
        stale.history.len(),
        2,
        "history rows outlive the definition"
    );
    let dropped = commit(&f.cron, |store| {
        store.prepare_delete_scope(CronScope::Workspace("ws-drift".into()))
    })
    .unwrap();
    assert_eq!(dropped.removed, [DRIFTED.to_string()]);
    assert!(!f
        .snapshot()
        .states
        .iter()
        .any(|state| state.job_id.as_str() == DRIFTED));
    let refused = commit(&f.cron, |store| store.prepare_delete(STALE, false));
    assert!(
        matches!(refused, Err(CronCommitError::Refused(_))),
        "{refused:?}"
    );
    f.assert_cache_is_committed();
}
