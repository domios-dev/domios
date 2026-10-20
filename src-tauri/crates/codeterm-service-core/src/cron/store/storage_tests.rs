//! storage_cron_: the cron domain against SQLite through its production prepare, commit
//! and publish path. Ordering comes from writer holds and stage gates, never from sleeps.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use parking_lot::Mutex;

use super::storage::{CronStage, StageProbe};
use super::{
    commit, decode_legacy_cron_file, finish_run, read_legacy_cron_file, write_log, CronCommitError,
    CronCommitted, CronCreateParams, CronJob, CronRunInfo, CronRunStatus, CronScope,
    CronStorageOptions, CronStore, CronUpdateParams, LegacyCronError, RunSession,
    MAX_CONSECUTIVE_FAILURES,
};
use crate::cron::clock::MockClock;
use crate::cron::scheduler::{record_fire_start, CronState};
use crate::storage::{
    CronJobId, CronJobRecord, CronRunId, CronRunRecord, CronRunState, CronSnapshot, Fault, Hold,
    Storage, StorageConfig, StorageError,
};

mod concurrency;
mod divergence;

/// Failure bound around event waits only; elapsed time never signals readiness.
const DEADLINE: Duration = Duration::from_secs(30);
const START_MS: u64 = 1_700_000_000_000;

/// Fields drop in order: Storage settles its owner before the temp root goes away.
struct Fixture {
    cron: CronState,
    storage: Storage,
    clock: Arc<MockClock>,
    temp: tempfile::TempDir,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.storage.shutdown().wait(DEADLINE);
    }
}

fn instance(temp: &tempfile::TempDir) -> PathBuf {
    temp.path().join("instance")
}

fn hydrate(storage: &Storage, temp: &tempfile::TempDir, clock: &Arc<MockClock>) -> CronStore {
    CronStore::open_storage(
        storage,
        temp.path().join("crons"),
        clock.clone(),
        CronStorageOptions::default(),
        crate::config::schema::AppConfig::default()
            .general_agent
            .agent_provider,
    )
    .expect("hydrate cron storage")
}

fn fixture_with(config: StorageConfig) -> Fixture {
    let temp = tempfile::tempdir().expect("temp root");
    let storage = Storage::open(instance(&temp), config).expect("open storage");
    let clock = MockClock::starting_at(START_MS);
    let store = hydrate(&storage, &temp, &clock);
    Fixture {
        cron: Arc::new(Mutex::new(store)),
        storage,
        temp,
        clock,
    }
}

fn fixture() -> Fixture {
    fixture_with(StorageConfig::default())
}

impl Fixture {
    fn rehydrated(&self) -> CronStore {
        hydrate(&self.storage, &self.temp, &self.clock)
    }

    fn snapshot(&self) -> CronSnapshot {
        self.storage
            .crons()
            .snapshot()
            .wait(DEADLINE)
            .expect("committed snapshot")
            .value
    }

    /// The cache holds exactly what a fresh hydration from committed rows holds.
    fn assert_cache_is_committed(&self) {
        let fresh = self.rehydrated();
        let cache = self.cron.lock();
        assert_eq!(cache.list(), fresh.list(), "definitions and order");
        assert_eq!(histories(&cache), histories(&fresh), "run history");
    }

    fn create(&self, params: CronCreateParams) -> CronJob {
        commit(&self.cron, |store| store.prepare_create(params.clone()))
            .expect("create")
            .value
    }

    fn start(&self, job_id: &str, run_id: &str) {
        commit(&self.cron, |store| {
            store.prepare_record_run(job_id, running(run_id))
        })
        .expect("record run start");
    }

    fn name_of(&self, job_id: &str) -> String {
        self.cron
            .lock()
            .get(job_id)
            .expect("cached job")
            .name
            .clone()
    }
}

fn histories(store: &CronStore) -> BTreeMap<String, Vec<CronRunInfo>> {
    store
        .test_history_snapshot()
        .iter()
        .filter(|(_, runs)| !runs.is_empty())
        .map(|(id, runs)| (id.clone(), runs.clone()))
        .collect()
}

fn params(name: &str) -> CronCreateParams {
    serde_json::from_value(serde_json::json!({
        "name": name,
        "workspace_id": "ws",
        "schedule": "0 * * * *",
        "task": "task",
        "agent": "claude",
        "jitter_max_ms": 0,
    }))
    .expect("create params")
}

fn scoped(name: &str, workspace: &str, team: Option<&str>) -> CronCreateParams {
    CronCreateParams {
        workspace_id: workspace.to_string(),
        team_id: team.map(str::to_string),
        ..params(name)
    }
}

fn rename(name: &str) -> CronUpdateParams {
    serde_json::from_value(serde_json::json!({ "name": name })).expect("rename patch")
}

fn running(run_id: &str) -> CronRunInfo {
    CronRunInfo {
        run_id: run_id.to_string(),
        started_at: START_MS,
        finished_at: None,
        status: CronRunStatus::Running,
        tab_id: Some(format!("tab-{run_id}")),
        session_id: None,
        cwd: None,
        summary: None,
        report_delivery: None,
    }
}

fn held(storage: &Storage) -> Hold {
    let hold = storage.hold_writer().expect("writer hold admitted");
    hold.wait_held(DEADLINE).expect("writer reached the hold");
    hold
}

/// Blocks the first commit that reaches `stage` until released; later commits pass.
struct Gate {
    reached: mpsc::Receiver<CronStage>,
    release: mpsc::Sender<()>,
}

fn gate(cron: &CronState, stage: CronStage) -> Gate {
    let mut armed = gates(cron, &[stage]);
    armed.pop().expect("one gate")
}

/// One gate per stage, each armed for the first commit reaching it.
fn gates(cron: &CronState, stages: &[CronStage]) -> Vec<Gate> {
    let mut armed = Vec::new();
    let mut handles = Vec::new();
    for stage in stages {
        let (reached_tx, reached) = mpsc::channel();
        let (release, release_rx) = mpsc::channel::<()>();
        armed.push((*stage, Mutex::new(Some((reached_tx, release_rx)))));
        handles.push(Gate { reached, release });
    }
    let probe: StageProbe = Arc::new(move |hit: CronStage| {
        let taken = armed
            .iter()
            .find(|(stage, _)| *stage == hit)
            .and_then(|(_, slot)| slot.lock().take());
        if let Some((reached, release)) = taken {
            let _ = reached.send(hit);
            let _ = release.recv();
        }
    });
    cron.lock().set_stage_probe(probe);
    handles
}

impl Gate {
    fn wait_reached(&self) {
        self.reached
            .recv_timeout(DEADLINE)
            .expect("a commit reached the gate");
    }

    fn release(self) {
        let _ = self.release.send(());
    }
}

fn state_of<'a>(snapshot: &'a CronSnapshot, job_id: &str) -> &'a crate::storage::CronJobState {
    snapshot
        .states
        .iter()
        .find(|state| state.job_id.as_str() == job_id)
        .expect("stored identity")
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            found.extend(json_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "json") {
            found.push(path);
        }
    }
    found
}

#[test]
fn storage_cron_every_field_and_order_roundtrips() {
    let f = fixture();
    let full = f.create(CronCreateParams {
        name: "full".into(),
        workspace_id: "ws-full".into(),
        team_id: Some("team-1".into()),
        detach_from_team: false,
        schedule: "*/5 * * * *".into(),
        task: "full task".into(),
        description: Some(String::new()),
        agent: Some("codex".into()),
        model: Some("model-x".into()),
        reasoning_effort: Some(" high ".into()),
        cwd: Some("/work/full".into()),
        role: Some("role".into()),
        skill: Some("skill".into()),
        creator_tab_id: Some("creator-tab".into()),
        report_target_tab_id: Some("report-tab".into()),
        enabled: Some(true),
        max_history: Some(2),
        oneshot: Some(false),
        jitter_max_ms: Some(0),
        tz: Some("Europe/Riga".into()),
        timeout_seconds: Some(900),
    });
    let bare = f.create(params("bare"));
    let third = f.create(params("third"));
    for run_id in ["r1", "r2", "r3"] {
        f.clock.advance_ms(60_000);
        f.start(&full.id, run_id);
    }
    commit(&f.cron, |store| {
        Ok(store.prepare_set_run_tab(&full.id, "r3", "spawned-tab"))
    })
    .expect("run tab");
    let session = RunSession {
        session_id: "session-3",
        cwd: Some("/work/run"),
    };
    commit(&f.cron, |store| {
        Ok(store.prepare_set_run_session(&full.id, "r3", session))
    })
    .expect("run session");
    assert!(finish_run(&f.cron, &full.id, "r2", CronRunStatus::Failed, None).expect("finish"));
    commit(&f.cron, |store| {
        Ok(store.prepare_set_run_report(&full.id, "r2", "summary", "delivered"))
    })
    .expect("run report");

    f.assert_cache_is_committed();
    let fresh = f.rehydrated();
    let order: Vec<&str> = fresh.list().iter().map(|job| job.id.as_str()).collect();
    assert_eq!(
        order,
        [full.id.as_str(), bare.id.as_str(), third.id.as_str()]
    );
    let stored = fresh.get(&full.id).expect("full job");
    assert_eq!(stored.description.as_deref(), Some(""), "empty is not null");
    assert_eq!(stored.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(stored.tz.as_deref(), Some("Europe/Riga"));
    assert_eq!(
        (
            stored.timeout_seconds,
            stored.max_history,
            stored.consecutive_failures
        ),
        (Some(900), 2, 1)
    );
    let history = fresh.get_history(&full.id);
    let ids: Vec<&str> = history.iter().map(|run| run.run_id.as_str()).collect();
    assert_eq!(ids, ["r2", "r3"], "the oldest run was trimmed in order");
    assert_eq!(history[0].report_delivery.as_deref(), Some("delivered"));
    let last = stored.last_run.as_ref().expect("last run");
    assert_eq!(
        (
            last.tab_id.as_deref(),
            last.session_id.as_deref(),
            last.cwd.as_deref()
        ),
        (Some("spawned-tab"), Some("session-3"), Some("/work/run"))
    );
    assert_eq!(history[1], last, "the last run is its history row");
    let bare = fresh.get(&bare.id).expect("bare job");
    assert_eq!(
        (&bare.description, &bare.team_id, &bare.last_run),
        (&None, &None, &None)
    );
}

const LEGACY_FILE: &str = r#"{
  "version": "1",
  "jobs": [
    {
      "id": "job-a", "name": "full", "description": "", "workspace_id": "ws-a",
      "team_id": "team-a", "schedule": "0 6 * * *", "enabled": true, "agent": "codex",
      "model": "model-a", "reasoning_effort": "high", "task": "task a", "cwd": "/a",
      "role": "role-a", "skill": "skill-a", "creator_tab_id": "tab-gone",
      "report_target_tab_id": "tab-report-gone", "max_history": 5,
      "created_at": 1690000000000, "next_run": 1700000300000, "jitter_max_ms": 1234,
      "tz": "Europe/Riga", "timeout_seconds": 600, "consecutive_failures": 2,
      "protected": true,
      "last_run": {"run_id": "r2", "started_at": 20, "finished_at": null,
        "status": "Running", "tab_id": "tab-never-registered"}
    },
    {
      "id": "job-b", "name": "minimal", "workspace_id": "", "schedule": "in 1h",
      "enabled": false, "agent": "claude", "task": "task b", "created_at": 1,
      "oneshot": true,
      "last_run": {"run_id": "r9", "started_at": 90, "finished_at": 91,
        "status": "Success", "tab_id": null, "summary": "kept only as last run"}
    },
    {
      "id": "job-c", "name": "never ran", "workspace_id": "ws-c",
      "schedule": "*/5 * * * *", "enabled": true, "agent": "claude", "task": "task c",
      "created_at": 2, "max_history": 0
    }
  ],
  "history": {
    "job-a": [
      {"run_id": "r1", "started_at": 10, "finished_at": 11, "status": "Success",
        "tab_id": "tab-1", "session_id": "session-1", "cwd": "/a/run",
        "summary": "done", "report_delivery": "delivered"},
      {"run_id": "r2", "started_at": 20, "finished_at": null, "status": "Running",
        "tab_id": "tab-never-registered"}
    ],
    "ghost": [
      {"run_id": "r7", "started_at": 70, "finished_at": 71, "status": "Failed",
        "tab_id": null}
    ]
  }
}"#;

#[test]
fn storage_cron_legacy_import_conserves_every_original_fact() {
    let f = fixture();
    let originals = f.temp.path().join("originals");
    std::fs::create_dir_all(&originals).expect("originals dir");
    let path = originals.join("crons.json");
    std::fs::write(&path, LEGACY_FILE).expect("write original");

    let import = read_legacy_cron_file(&path)
        .expect("decodes")
        .expect("present");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), LEGACY_FILE);
    assert_eq!(
        json_files(&originals),
        [path.clone()],
        "nothing renamed or copied"
    );
    f.storage
        .crons()
        .import(import.clone())
        .wait(DEADLINE)
        .expect("import into the empty cron domain");

    let legacy = CronStore::new_with_path(path.clone());
    let imported = f.rehydrated();
    assert_eq!(
        imported.list(),
        legacy.list(),
        "every field, null and order"
    );
    assert_eq!(histories(&imported), histories(&legacy));
    assert!(
        imported.get("ghost").is_none(),
        "history alone is no live job"
    );
    assert_eq!(imported.get_history("ghost").len(), 1);
    assert!(state_of(&f.snapshot(), "ghost").identity.retired);
    assert!(
        imported.test_tab_owner_count() == 0 && !imported.is_job_running("job-a"),
        "a historical Running row never becomes live tab ownership"
    );
    let before = f.snapshot();
    let again = f.storage.crons().import(import).wait(DEADLINE);
    assert!(
        matches!(again, Err(StorageError::Invalid { .. })),
        "{again:?}"
    );
    assert_eq!(f.snapshot(), before, "a repeated import changes nothing");
}

#[test]
fn storage_cron_legacy_decoder_refuses_without_touching_originals() {
    let temp = tempfile::tempdir().expect("temp root");
    assert_eq!(
        read_legacy_cron_file(&temp.path().join("absent.json")),
        Ok(None)
    );
    let unnamed = r#"{"version": "1", "jobs": [{"id": "", "name": "n",
        "workspace_id": "w", "schedule": "0 * * * *", "enabled": true, "agent": "a",
        "task": "t", "created_at": 1}], "history": {}}"#;
    let repeated = r#"{"version": "1", "jobs": [], "history": {"j": [
        {"run_id": "r", "started_at": 1, "status": "Success", "tab_id": null},
        {"run_id": "r", "started_at": 2, "status": "Success", "tab_id": null}]}}"#;
    let cases: [(&str, &str, fn(&LegacyCronError) -> bool); 5] = [
        (
            "truncated",
            r#"{"version": "1", "jobs": ["#,
            |e: &LegacyCronError| matches!(e, LegacyCronError::Corrupt(_)),
        ),
        (
            "future",
            r#"{"version": "2", "jobs": [], "history": {}}"#,
            |e: &LegacyCronError| matches!(e, LegacyCronError::Unsupported(_)),
        ),
        (
            "versionless",
            r#"{"jobs": [], "history": {}}"#,
            |e: &LegacyCronError| matches!(e, LegacyCronError::Corrupt(_)),
        ),
        ("unnamed", unnamed, |e: &LegacyCronError| {
            matches!(e, LegacyCronError::Conflict(_))
        }),
        ("repeated", repeated, |e: &LegacyCronError| {
            matches!(e, LegacyCronError::Conflict(_))
        }),
    ];
    for (name, text, expected) in cases {
        let path = temp.path().join(format!("{name}.json"));
        std::fs::write(&path, text).expect("write original");
        let refused = read_legacy_cron_file(&path).expect_err(name);
        assert!(expected(&refused), "{name}: {refused:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "{name}");
        assert_eq!(
            decode_legacy_cron_file(text.as_bytes()).expect_err(name),
            refused
        );
    }
    assert_eq!(
        json_files(temp.path()).len(),
        5,
        "no recovery copy was written"
    );
}

fn rows<T>(done: CronCommitted<T>) -> u64 {
    done.receipt.expect("storage receipt").rows_written
}

/// Rows written by rename, run start, session update and failed completion.
fn rows_written(f: &Fixture, inventory: usize) -> [u64; 4] {
    for index in 0..inventory {
        f.create(params(&format!("filler-{index}")));
    }
    let job = f.create(params("measured"));
    let renamed = rows(
        commit(&f.cron, |store| {
            store.prepare_update(&job.id, rename("renamed"))
        })
        .expect("rename"),
    );
    let started = rows(
        commit(&f.cron, |store| {
            store.prepare_record_run(&job.id, running("r1"))
        })
        .expect("start"),
    );
    let session = RunSession {
        session_id: "s",
        cwd: None,
    };
    let bound = rows(
        commit(&f.cron, |store| {
            Ok(store.prepare_set_run_session(&job.id, "r1", session))
        })
        .expect("session"),
    );
    let failed = rows(
        commit(&f.cron, |store| {
            store.prepare_finish_run(&job.id, "r1", CronRunStatus::Failed, None)
        })
        .expect("finish"),
    );
    [renamed, started, bound, failed]
}

#[test]
fn storage_cron_updates_write_bounded_rows_independent_of_inventory() {
    let small = rows_written(&fixture(), 0);
    let large = rows_written(&fixture(), 64);
    assert_eq!(small, large, "rows written never scale with the inventory");
    assert_eq!(small, [2, 3, 2, 3]);
}

#[test]
fn storage_cron_completion_accounting_is_once_only() {
    let f = fixture();
    let job = f.create(params("flaky"));
    f.start(&job.id, "r1");
    assert!(finish_run(&f.cron, &job.id, "r1", CronRunStatus::Failed, None).unwrap());
    assert!(
        !finish_run(&f.cron, &job.id, "r1", CronRunStatus::Failed, None).unwrap(),
        "a repeated completion is not fresh"
    );
    assert_eq!(f.rehydrated().get(&job.id).unwrap().consecutive_failures, 1);
    for index in 2..=MAX_CONSECUTIVE_FAILURES {
        let run_id = format!("r{index}");
        f.start(&job.id, &run_id);
        assert!(finish_run(&f.cron, &job.id, &run_id, CronRunStatus::Failed, None).unwrap());
    }
    let disabled = f.rehydrated().get(&job.id).cloned().unwrap();
    assert_eq!(
        (
            disabled.consecutive_failures,
            disabled.enabled,
            disabled.next_run
        ),
        (MAX_CONSECUTIVE_FAILURES, false, None)
    );
    f.start(&job.id, "recovered");
    assert!(finish_run(&f.cron, &job.id, "recovered", CronRunStatus::Success, None).unwrap());
    assert_eq!(f.rehydrated().get(&job.id).unwrap().consecutive_failures, 0);
    f.assert_cache_is_committed();
}

#[test]
fn storage_cron_oneshot_paused_and_protected_state_persists() {
    let f = fixture();
    let once = f.create(CronCreateParams {
        schedule: "in 1h".into(),
        oneshot: Some(true),
        ..params("once")
    });
    f.start(&once.id, "o1");
    let paused = f.create(params("paused"));
    commit(&f.cron, |store| {
        store.prepare_set_enabled(&paused.id, false)
    })
    .unwrap();
    let guarded = f.create(params("guarded"));
    commit(&f.cron, |store| {
        store.prepare_set_protected(&guarded.id, true)
    })
    .unwrap();

    let before = f.snapshot();
    let refused = commit(&f.cron, |store| store.prepare_delete(&guarded.id, false));
    assert!(
        matches!(refused, Err(CronCommitError::Refused(_))),
        "{refused:?}"
    );
    assert_eq!(f.snapshot(), before, "a refused delete writes nothing");

    let fresh = f.rehydrated();
    let once = fresh.get(&once.id).unwrap();
    assert_eq!((once.enabled, once.next_run), (false, None));
    let paused = fresh.get(&paused.id).unwrap();
    assert_eq!((paused.enabled, paused.next_run), (false, None));
    assert!(fresh.get(&guarded.id).unwrap().protected);
    f.assert_cache_is_committed();
}

fn record(id: &str) -> CronJobRecord {
    CronJobRecord {
        id: CronJobId::new(id).unwrap(),
        name: "reuse".into(),
        description: None,
        workspace_id: "ws".into(),
        team_id: None,
        schedule: "0 * * * *".into(),
        enabled: true,
        agent: "claude".into(),
        model: None,
        reasoning_effort: None,
        task: "task".into(),
        cwd: None,
        role: None,
        skill: None,
        creator_tab_id: None,
        report_target_tab_id: None,
        oneshot: false,
        max_history: 10,
        created_at: 1,
        last_run_id: None,
        next_run: None,
        jitter_max_ms: None,
        tz: None,
        timeout_seconds: None,
        consecutive_failures: 0,
        protected: false,
    }
}

#[test]
fn storage_cron_deleted_job_history_is_a_retired_identity() {
    let f = fixture();
    let job = f.create(params("retiring"));
    f.start(&job.id, "r1");
    assert!(finish_run(&f.cron, &job.id, "r1", CronRunStatus::Success, None).unwrap());
    commit(&f.cron, |store| store.prepare_delete(&job.id, true)).expect("retire");

    let late = RunSession {
        session_id: "late-session",
        cwd: None,
    };
    commit(&f.cron, |store| {
        Ok(store.prepare_set_run_session(&job.id, "r1", late))
    })
    .expect("a retired run still records its session");
    assert!(
        !finish_run(&f.cron, &job.id, "r1", CronRunStatus::Success, None).unwrap(),
        "no definition is left to account"
    );
    let fresh = f.rehydrated();
    assert!(fresh.get(&job.id).is_none());
    let history = fresh.get_history(&job.id);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].session_id.as_deref(), Some("late-session"));
    let snapshot = f.snapshot();
    let retired = state_of(&snapshot, &job.id);
    assert!(retired.identity.retired && retired.job.is_none());
    let reused = f.storage.crons().create(record(&job.id)).wait(DEADLINE);
    assert!(
        matches!(reused, Err(StorageError::StaleRevision { .. })),
        "a retired identity is never a live cron again: {reused:?}"
    );

    let dropped = f.create(params("dropped"));
    f.start(&dropped.id, "d1");
    let done = commit(&f.cron, |store| store.prepare_delete(&dropped.id, false)).unwrap();
    assert_eq!(done.removed, [dropped.id.clone()]);
    assert!(f.cron.lock().get_history(&dropped.id).is_empty());
    assert!(!f
        .snapshot()
        .states
        .iter()
        .any(|state| state.job_id.as_str() == dropped.id));
    f.assert_cache_is_committed();
}

#[test]
fn storage_cron_unknown_identity_is_refused_and_historical_targets_stay_verbatim() {
    let f = fixture();
    let before = f.snapshot();
    let ghost_run = CronRunRecord {
        run_id: CronRunId::new("ghost-run").unwrap(),
        started_at: 1,
        finished_at: None,
        status: CronRunState::Running,
        tab_id: None,
        session_id: None,
        cwd: None,
        summary: None,
        report_delivery: None,
    };
    let refused = f
        .storage
        .crons()
        .update_runs(
            CronJobId::new("never-existed").unwrap(),
            f.storage.revision(),
            vec![ghost_run],
            None,
        )
        .wait(DEADLINE);
    assert!(
        matches!(
            refused,
            Err(StorageError::StaleRevision { actual: None, .. })
        ),
        "{refused:?}"
    );
    assert!(!finish_run(
        &f.cron,
        "never-existed",
        "ghost-run",
        CronRunStatus::Failed,
        None
    )
    .unwrap());
    assert_eq!(
        f.snapshot(),
        before,
        "nothing is fabricated for an unknown job"
    );

    let job = f.create(CronCreateParams {
        creator_tab_id: Some("tab-closed-long-ago".into()),
        report_target_tab_id: Some("tab-report-gone".into()),
        ..scoped("orphaned targets", "workspace-gone", Some("team-gone"))
    });
    f.start(&job.id, "r1");
    let fresh = f.rehydrated();
    let stored = fresh.get(&job.id).unwrap();
    assert_eq!(
        (
            stored.workspace_id.as_str(),
            stored.team_id.as_deref(),
            stored.creator_tab_id.as_deref(),
            stored.report_target_tab_id.as_deref(),
        ),
        (
            "workspace-gone",
            Some("team-gone"),
            Some("tab-closed-long-ago"),
            Some("tab-report-gone")
        )
    );
    assert_eq!(
        fresh.get_history(&job.id)[0].tab_id.as_deref(),
        Some("tab-r1")
    );
    assert!(fresh.get_tab_owner("tab-r1").is_none() && !fresh.is_job_running(&job.id));
}

#[test]
fn storage_cron_commit_failure_conserves_cache_and_database_and_never_dispatches() {
    let f = fixture();
    let job = f.create(params("guarded dispatch"));
    let cached = f.cron.lock().get(&job.id).cloned();
    let before = f.snapshot();
    f.storage
        .inject_fault(Fault::FailNextCommit)
        .wait(DEADLINE)
        .expect("arm commit fault");

    assert!(
        !record_fire_start(&f.cron, &job.id, "refused-run"),
        "an unrecorded run never dispatches"
    );
    assert_eq!(f.cron.lock().get(&job.id).cloned(), cached);
    assert!(f.cron.lock().get_history(&job.id).is_empty());
    assert_eq!(f.snapshot(), before);

    assert!(record_fire_start(&f.cron, &job.id, "recorded-run"));
    let last = f
        .cron
        .lock()
        .get(&job.id)
        .unwrap()
        .last_run
        .clone()
        .unwrap();
    assert_eq!(last.run_id, "recorded-run");
    f.assert_cache_is_committed();
}

#[test]
fn storage_cron_legacy_and_storage_backends_never_cross() {
    let f = fixture();
    let before = f.storage.revision();
    std::fs::create_dir_all(f.temp.path().join("legacy")).expect("legacy dir");
    let mut legacy = CronStore::new_with_path(f.temp.path().join("legacy").join("crons.json"));
    let legacy_job = legacy.create(params("json only")).expect("legacy create");
    legacy
        .record_run(&legacy_job.id, running("json-run"))
        .expect("legacy run");
    assert_eq!(
        f.storage.revision(),
        before,
        "the JSON authority never writes Storage"
    );
    assert!(f.snapshot().states.is_empty());

    let job = f.create(params("storage only"));
    f.start(&job.id, "storage-run");
    let files = json_files(f.temp.path());
    assert!(
        files
            .iter()
            .all(|file| file.starts_with(f.temp.path().join("legacy"))),
        "the Storage backend never writes JSON: {files:?}"
    );
    let mut store = f.cron.lock();
    assert!(store.save().is_err());
    assert!(
        store.create(params("bypass")).is_err(),
        "no lock-held JSON path"
    );
    assert_eq!(store.list().len(), 1);
}

#[test]
fn storage_cron_previous_schema_upgrades_in_place() {
    let temp = tempfile::tempdir().expect("temp root");
    // An earlier install already owns its instance root; the seed only adds `storage/`.
    std::fs::create_dir_all(instance(&temp)).expect("existing instance root");
    let seeded = Storage::seed_before_cron_schema(&instance(&temp)).expect("seed");
    assert!(seeded > 0, "the seed carries the earlier schema");
    let storage = Storage::open(instance(&temp), StorageConfig::default()).expect("upgrade");
    let clock = MockClock::starting_at(START_MS);
    let cron: CronState = Arc::new(Mutex::new(hydrate(&storage, &temp, &clock)));
    assert!(cron.lock().list().is_empty());
    commit(&cron, |store| store.prepare_create(params("after upgrade"))).expect("create");
    storage
        .workspaces()
        .list()
        .wait(DEADLINE)
        .expect("earlier tables still serve");
    storage.shutdown().wait(DEADLINE).expect("shutdown");
    let reopened = Storage::open(instance(&temp), StorageConfig::default()).expect("reopen");
    assert_eq!(hydrate(&reopened, &temp, &clock).list().len(), 1);
}

#[test]
fn storage_cron_scope_removal_drops_scope_jobs_and_their_history() {
    let f = fixture();
    let a1 = f.create(scoped("ws-a-general", "ws-a", None));
    let a2 = f.create(scoped("ws-a-team", "ws-a", Some("team-x")));
    let b = f.create(scoped("ws-b-team", "ws-b", Some("team-x")));
    let kept = f.create(scoped("ws-b-kept", "ws-b", None));
    for job in [&a1, &a2, &b, &kept] {
        f.start(&job.id, &format!("run-{}", job.name));
    }
    let workspace = commit(&f.cron, |store| {
        store.prepare_delete_scope(CronScope::Workspace("ws-a".into()))
    })
    .unwrap();
    assert_eq!(workspace.removed, [a1.id.clone(), a2.id.clone()]);
    let team = commit(&f.cron, |store| {
        store.prepare_delete_scope(CronScope::Team("team-x".into()))
    })
    .unwrap();
    assert_eq!(team.removed, [b.id.clone()]);
    let store = f.rehydrated();
    let left: Vec<&str> = store.list().iter().map(|job| job.id.as_str()).collect();
    assert_eq!(left, [kept.id.as_str()]);
    for removed in [&a1, &a2, &b] {
        assert!(store.get_history(&removed.id).is_empty());
    }
    f.assert_cache_is_committed();
}

#[test]
fn storage_cron_logs_stay_files_named_by_plain_ids() {
    let f = fixture();
    let job = f.create(params("logged"));
    f.start(&job.id, "r1");
    let path = f.cron.lock().log_file(&job.id, "r1").expect("log path");
    write_log(&path, "log body").expect("write log");
    assert!(path.starts_with(f.temp.path().join("crons").join(&job.id)));
    assert_eq!(
        f.cron.lock().read_log(&job.id, "r1").as_deref(),
        Some("log body")
    );
    for bad in ["", ".", "..", "../escape", "a/b", "a\\b"] {
        let store = f.cron.lock();
        assert!(store.log_file(bad, "r1").is_err(), "job id {bad:?}");
        assert!(store.log_file(&job.id, bad).is_err(), "run id {bad:?}");
        assert!(store.job_logs_dir(bad).is_none(), "job id {bad:?}");
    }
    let snapshot = f.snapshot();
    let row = &state_of(&snapshot, &job.id).history[0];
    assert_eq!(
        (row.summary.as_deref(), row.report_delivery.as_deref()),
        (None, None)
    );

    f.storage
        .inject_fault(Fault::FailNextCommit)
        .wait(DEADLINE)
        .expect("arm commit fault");
    let refused = commit(&f.cron, |store| store.prepare_delete(&job.id, false));
    assert!(
        matches!(refused, Err(CronCommitError::NotApplied(_))),
        "{refused:?}"
    );
    let removed = commit(&f.cron, |store| store.prepare_delete(&job.id, false))
        .expect("delete commits")
        .removed;
    assert_eq!(
        removed,
        [job.id.clone()],
        "only committed removals release logs"
    );
    assert!(path.exists(), "the store itself never unlinks a log");
}

/// Desktop-runtime consumers: these run where ServiceFixture runs, like their siblings.
mod consumers {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::*;
    use crate::cron::scheduler::CronState;
    use crate::cron::store::{CronRunStatus, CronStore};
    use crate::service::test_fixture::ServiceFixture;

    fn legacy_service() -> (ServiceFixture, CronState, crate::CronWake) {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        let path = fixture.temp.path().join("crons.json");
        let cron: CronState = Arc::new(Mutex::new(CronStore::new_with_path(path)));
        (fixture, cron, Arc::new(std::sync::Condvar::new()))
    }

    #[test]
    fn storage_cron_consumer_protected_delete_keeps_logs() {
        let (fixture, cron, wake) = legacy_service();
        let id = {
            let mut store = cron.lock();
            let job = store.create(params("protected")).unwrap();
            store.record_run(&job.id, running("r1")).unwrap();
            store.save_log(&job.id, "r1", "kept log").unwrap();
            store.set_protected(&job.id, true).unwrap();
            job.id
        };
        assert!(fixture.service.cron_delete(&cron, &wake, &id).is_err());
        let store = cron.lock();
        assert!(store.get(&id).is_some());
        assert_eq!(store.read_log(&id, "r1").as_deref(), Some("kept log"));
    }

    #[test]
    fn storage_cron_consumer_cancel_marks_the_last_run_too() {
        let (fixture, cron, _) = legacy_service();
        let id = {
            let mut store = cron.lock();
            let job = store.create(params("cancelled")).unwrap();
            store.record_run(&job.id, running("r1")).unwrap();
            job.id
        };
        fixture.service.cron_cancel_running(&cron, &id).unwrap();
        let store = cron.lock();
        assert_eq!(store.get_history(&id)[0].status, CronRunStatus::Cancelled);
        let last = store.get(&id).unwrap().last_run.clone().unwrap();
        assert_eq!(last.status, CronRunStatus::Cancelled, "one run, one status");
        assert_eq!(Some(&last), store.get_history(&id).first().copied());
    }
}
