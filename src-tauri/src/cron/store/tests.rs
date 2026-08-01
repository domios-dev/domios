use super::*;

fn temp_store() -> CronStore {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crons.json");
    let path_clone = path.clone();
    std::mem::forget(dir);
    CronStore::new_with_path(path_clone)
}

fn make_params(name: &str, ws: &str, schedule: &str) -> CronCreateParams {
    CronCreateParams {
        name: name.to_string(),
        workspace_id: ws.to_string(),
        team_id: None,
        schedule: schedule.to_string(),
        task: "test task".to_string(),
        description: None,
        agent: Some("claude".to_string()),
        model: None,
        cwd: None,
        role: None,
        skill: None,
        creator_tab_id: None,
        report_target_tab_id: None,
        enabled: None,
        max_history: None,
        oneshot: None,
        jitter_max_ms: Some(0),
        tz: None,
        timeout_seconds: None,
    }
}

#[test]
fn set_run_session_persists_on_history_and_last_run() {
    let mut store = temp_store();
    let job = store.create(make_params("T", "ws", "0 * * * *")).unwrap();
    let run = CronRunInfo {
        run_id: "r1".into(),
        started_at: 1,
        finished_at: None,
        status: CronRunStatus::Running,
        tab_id: Some("p1".into()),
        session_id: None,
        cwd: None,
    };
    store.record_run(&job.id, run).unwrap();
    store.set_run_session(&job.id, "r1", "sid-9", Some("/cwd"));
    let hist = store.get_history(&job.id);
    let saved = hist.iter().find(|r| r.run_id == "r1").unwrap();
    assert_eq!(saved.session_id.as_deref(), Some("sid-9"));
    assert_eq!(saved.cwd.as_deref(), Some("/cwd"));
    assert_eq!(
        store
            .get(&job.id)
            .unwrap()
            .last_run
            .as_ref()
            .unwrap()
            .session_id
            .as_deref(),
        Some("sid-9")
    );
}

#[test]
fn team_jobs_keep_ownership_and_report_destination_separate() {
    let mut store = temp_store();
    let mut params = make_params("team-job", "workspace", "0 * * * *");
    params.team_id = Some("team".into());
    params.creator_tab_id = Some("creator".into());
    params.report_target_tab_id = Some("target".into());

    let job = store.create(params).unwrap();

    assert_eq!(job.team_id.as_deref(), Some("team"));
    assert_eq!(job.creator_tab_id.as_deref(), Some("creator"));
    assert_eq!(job.report_target_tab_id.as_deref(), Some("target"));
    assert_eq!(store.list_for_team("team").len(), 1);
    assert!(store.list_for_team("other").is_empty());
}

#[test]
fn protected_jobs_cannot_be_deleted() {
    let mut store = temp_store();
    let job = store.create(make_params("system job", "workspace", "0 * * * *")).unwrap();
    store.get_mut(&job.id).unwrap().protected = true;

    let error = store.delete(&job.id).unwrap_err();

    assert!(error.contains("protected"));
    assert!(store.get(&job.id).is_some());
}

#[test]
fn record_failure_increments_counter_and_backs_off() {
    let mut store = temp_store();
    let job = store
        .create(make_params("Test", "ws", "0 * * * *"))
        .unwrap();
    let initial_next = job.next_run.unwrap();

    store
        .record_run(
            &job.id,
            CronRunInfo {
                run_id: "r1".into(),
                started_at: 0,
                finished_at: Some(0),
                status: CronRunStatus::Failed,
                tab_id: None,
                session_id: None,
                cwd: None,
            },
        )
        .unwrap();

    let after = store.get(&job.id).unwrap();
    assert_eq!(after.consecutive_failures, 1);
    let _ = initial_next;
}

#[test]
fn record_5_failures_auto_disables_job() {
    let mut store = temp_store();
    let job = store
        .create(make_params("Test", "ws", "0 * * * *"))
        .unwrap();
    for i in 0..MAX_CONSECUTIVE_FAILURES {
        store
            .record_run(
                &job.id,
                CronRunInfo {
                    run_id: format!("r{}", i),
                    started_at: 0,
                    finished_at: Some(0),
                    status: CronRunStatus::Failed,
                    tab_id: None,
                    session_id: None,
                    cwd: None,
                },
            )
            .unwrap();
    }
    let after = store.get(&job.id).unwrap();
    assert!(
        !after.enabled,
        "should auto-disable after {} failures",
        MAX_CONSECUTIVE_FAILURES
    );
    assert_eq!(after.consecutive_failures, MAX_CONSECUTIVE_FAILURES);
}

#[test]
fn success_resets_failure_counter() {
    let mut store = temp_store();
    let job = store
        .create(make_params("Test", "ws", "0 * * * *"))
        .unwrap();
    for i in 0..3 {
        store
            .record_run(
                &job.id,
                CronRunInfo {
                    run_id: format!("f{}", i),
                    started_at: 0,
                    finished_at: Some(0),
                    status: CronRunStatus::Failed,
                    tab_id: None,
                    session_id: None,
                    cwd: None,
                },
            )
            .unwrap();
    }
    store
        .record_run(
            &job.id,
            CronRunInfo {
                run_id: "ok".into(),
                started_at: 0,
                finished_at: Some(0),
                status: CronRunStatus::Success,
                tab_id: None,
                session_id: None,
                cwd: None,
            },
        )
        .unwrap();
    let after = store.get(&job.id).unwrap();
    assert_eq!(after.consecutive_failures, 0);
    assert!(after.enabled);
}

#[test]
fn test_cron_crud() {
    let mut store = temp_store();

    let job = store
        .create(make_params("Test Cron", "ws-1", "*/30 * * * *"))
        .unwrap();
    assert_eq!(job.name, "Test Cron");
    assert_eq!(job.agent, "claude");
    assert!(job.enabled);
    assert!(job.next_run.is_some());

    let fetched = store.get(&job.id).unwrap();
    assert_eq!(fetched.name, "Test Cron");

    let updated = store
        .update(
            &job.id,
            CronUpdateParams {
                name: Some("Renamed".to_string()),
                description: None,
                schedule: None,
                task: None,
                cwd: None,
                role: None,
                skill: None,
                enabled: None,
                max_history: None,
                agent: None,
                model: None,
                jitter_max_ms: None,
                tz: None,
                timeout_seconds: None,
            },
        )
        .unwrap();
    assert_eq!(updated.name, "Renamed");

    store.delete(&job.id).unwrap();
    assert!(store.get(&job.id).is_none());
}

#[test]
fn is_job_running_tracks_registered_tabs() {
    let mut store = temp_store();
    let job = store
        .create(make_params("Runner", "ws-1", "0 * * * *"))
        .unwrap();
    assert!(!store.is_job_running(&job.id));

    store.register_tab("tab-A", &job.id, "run-1");
    assert!(store.is_job_running(&job.id));

    store.register_tab("tab-B", &job.id, "run-2");
    assert!(store.is_job_running(&job.id));

    store.unregister_tab("tab-A");
    assert!(store.is_job_running(&job.id));

    store.unregister_tab("tab-B");
    assert!(!store.is_job_running(&job.id));
}

#[test]
fn test_list_for_workspace() {
    let mut store = temp_store();
    store
        .create(make_params("Job A", "ws-1", "0 * * * *"))
        .unwrap();
    store
        .create(make_params("Job B", "ws-2", "0 * * * *"))
        .unwrap();
    store
        .create(make_params("Job C", "ws-1", "0 * * * *"))
        .unwrap();

    assert_eq!(store.list_for_workspace("ws-1").len(), 2);
    assert_eq!(store.list_for_workspace("ws-2").len(), 1);
    assert_eq!(store.list_for_workspace("ws-3").len(), 0);
}

#[test]
fn test_record_run_history() {
    let mut store = temp_store();
    let job = store
        .create(CronCreateParams {
            max_history: Some(3),
            ..make_params("Job", "ws-1", "0 * * * *")
        })
        .unwrap();

    for i in 0..5 {
        store
            .record_run(
                &job.id,
                CronRunInfo {
                    run_id: format!("run-{}", i),
                    started_at: 1000 + i,
                    finished_at: Some(2000 + i),
                    status: CronRunStatus::Success,
                    tab_id: None,
                    session_id: None,
                    cwd: None,
                },
            )
            .unwrap();
    }

    let history = store.get_history(&job.id);
    assert_eq!(history.len(), 3); // max_history eviction
    assert_eq!(history[0].run_id, "run-2"); // oldest kept
    assert_eq!(history[2].run_id, "run-4"); // newest

    let j = store.get(&job.id).unwrap();
    assert_eq!(j.last_run.as_ref().unwrap().run_id, "run-4");
}

#[test]
fn test_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crons.json");

    {
        let mut store = CronStore::new_with_path(path.clone());
        store
            .create(make_params("Persistent", "ws-1", "0 9 * * *"))
            .unwrap();
    }

    {
        let store = CronStore::new_with_path(path);
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].name, "Persistent");
    }
}

#[test]
fn test_oneshot_create_and_record_run() {
    let mut store = temp_store();

    let job = store
        .create(CronCreateParams {
            oneshot: Some(true),
            ..make_params("Once", "ws-1", "in 10m")
        })
        .unwrap();

    assert!(job.oneshot);
    assert!(job.enabled);
    assert!(job.next_run.is_some());

    store
        .record_run(
            &job.id,
            CronRunInfo {
                run_id: "run-1".into(),
                started_at: 1000,
                finished_at: Some(2000),
                status: CronRunStatus::Success,
                tab_id: None,
                session_id: None,
                cwd: None,
            },
        )
        .unwrap();

    let j = store.get(&job.id).unwrap();
    assert!(!j.enabled);
    assert!(j.next_run.is_none());
}

#[test]
fn create_accepts_tz_and_recomputes_next_run() {
    let mut store = temp_store();
    let job_utc = store
        .create(make_params("UTC job", "ws", "0 3 * * *"))
        .unwrap();
    let job_riga = store
        .create(CronCreateParams {
            tz: Some("Europe/Riga".into()),
            ..make_params("Riga job", "ws", "0 3 * * *")
        })
        .unwrap();
    assert_eq!(job_utc.tz, None);
    assert_eq!(job_riga.tz.as_deref(), Some("Europe/Riga"));
    // Same schedule in different tz must yield different fire time.
    assert_ne!(job_utc.next_run, job_riga.next_run);
}

#[test]
fn create_rejects_invalid_tz() {
    let mut store = temp_store();
    let r = store.create(CronCreateParams {
        tz: Some("Mars/Time".into()),
        ..make_params("Bad", "ws", "0 3 * * *")
    });
    assert!(r.is_err());
}

#[test]
fn create_empty_tz_normalizes_to_none() {
    let mut store = temp_store();
    let job = store
        .create(CronCreateParams {
            tz: Some("   ".into()),
            ..make_params("EmptyTz", "ws", "0 3 * * *")
        })
        .unwrap();
    assert_eq!(job.tz, None);
}

#[test]
fn create_accepts_timeout_seconds() {
    let mut store = temp_store();
    let none_job = store
        .create(make_params("NoTimeout", "ws", "0 * * * *"))
        .unwrap();
    let with_job = store
        .create(CronCreateParams {
            timeout_seconds: Some(1800),
            ..make_params("HasTimeout", "ws", "0 * * * *")
        })
        .unwrap();
    let zeroed = store
        .create(CronCreateParams {
            timeout_seconds: Some(0),
            ..make_params("ZeroTimeout", "ws", "0 * * * *")
        })
        .unwrap();
    assert_eq!(none_job.timeout_seconds, None);
    assert_eq!(with_job.timeout_seconds, Some(1800));
    // 0 means "no override" — normalized to None on create.
    assert_eq!(zeroed.timeout_seconds, None);
}

#[test]
fn update_can_set_clear_tz_and_timeout() {
    let mut store = temp_store();
    let job = store
        .create(make_params("ToPatch", "ws", "0 3 * * *"))
        .unwrap();

    let updated = store
        .update(
            &job.id,
            CronUpdateParams {
                name: None,
                description: None,
                schedule: None,
                task: None,
                cwd: None,
                role: None,
                skill: None,
                enabled: None,
                max_history: None,
                agent: None,
                model: None,
                jitter_max_ms: None,
                tz: Some("Europe/Riga".into()),
                timeout_seconds: Some(900),
            },
        )
        .unwrap();
    assert_eq!(updated.tz.as_deref(), Some("Europe/Riga"));
    assert_eq!(updated.timeout_seconds, Some(900));

    // Empty tz string clears back to UTC; timeout 0 clears override.
    let cleared = store
        .update(
            &job.id,
            CronUpdateParams {
                name: None,
                description: None,
                schedule: None,
                task: None,
                cwd: None,
                role: None,
                skill: None,
                enabled: None,
                max_history: None,
                agent: None,
                model: None,
                jitter_max_ms: None,
                tz: Some("".into()),
                timeout_seconds: Some(0),
            },
        )
        .unwrap();
    assert_eq!(cleared.tz, None);
    assert_eq!(cleared.timeout_seconds, None);
}

#[test]
fn update_rejects_invalid_tz() {
    let mut store = temp_store();
    let job = store.create(make_params("X", "ws", "0 3 * * *")).unwrap();
    let r = store.update(
        &job.id,
        CronUpdateParams {
            name: None,
            description: None,
            schedule: None,
            task: None,
            cwd: None,
            role: None,
            skill: None,
            enabled: None,
            max_history: None,
            agent: None,
            model: None,
            jitter_max_ms: None,
            tz: Some("Imaginary/Place".into()),
            timeout_seconds: None,
        },
    );
    assert!(r.is_err());
}

/// Regression for issue #97: editing only `schedule` (no tz, no jitter) must
/// recompute `next_run`, not leave it anchored to the previous schedule.
#[test]
fn update_schedule_only_recomputes_next_run() {
    let mut store = temp_store();
    let job = store
        .create(CronCreateParams {
            tz: Some("Europe/Riga".into()),
            jitter_max_ms: Some(0),
            ..make_params("DailyCron", "ws", "0 6 * * *")
        })
        .unwrap();
    let before = job.next_run.expect("created cron must have next_run");

    let updated = store
        .update(
            &job.id,
            CronUpdateParams {
                name: None,
                description: None,
                schedule: Some("0 3 * * *".into()),
                task: None,
                cwd: None,
                role: None,
                skill: None,
                enabled: None,
                max_history: None,
                agent: None,
                model: None,
                jitter_max_ms: None,
                tz: None,
                timeout_seconds: None,
            },
        )
        .unwrap();

    assert_eq!(updated.schedule, "0 3 * * *");
    assert_eq!(updated.tz.as_deref(), Some("Europe/Riga"));
    let after = updated
        .next_run
        .expect("next_run must remain set after edit");
    assert_ne!(
        before, after,
        "schedule-only edit must move next_run off the old schedule (was {}, still {})",
        before, after
    );
}

// ── Time-travel semantics (injected MockClock) ─────────────────────

use crate::cron::clock::MockClock;

/// 2023-11-14T22:13:20Z — fixed epoch so boundary math is deterministic.
const T0: u64 = 1_700_000_000_000;

fn mock_store(start_ms: u64) -> (CronStore, std::sync::Arc<MockClock>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crons.json");
    std::mem::forget(dir);
    let clock = MockClock::starting_at(start_ms);
    let store = CronStore::new_with_path_and_clock(path, clock.clone());
    (store, clock)
}

fn run(run_id: &str, started_at: u64, status: CronRunStatus) -> CronRunInfo {
    CronRunInfo {
        run_id: run_id.into(),
        started_at,
        finished_at: None,
        status,
        tab_id: None,
        session_id: None,
        cwd: None,
    }
}

#[test]
fn created_at_and_next_run_come_from_injected_clock() {
    let (mut store, _clock) = mock_store(T0);
    let job = store
        .create(make_params("clock", "ws", "*/5 * * * *"))
        .unwrap();
    assert_eq!(job.created_at, T0);
    // Next */5 boundary after 22:13:20 is 22:15:00.
    assert_eq!(job.next_run, Some(1_700_000_100_000));
}

#[test]
fn oneshot_in_30s_becomes_due_exactly_after_advance() {
    let (mut store, clock) = mock_store(T0);
    let mut params = make_params("oneshot", "ws", "in 30s");
    params.oneshot = Some(true);
    let job = store.create(params).unwrap();
    assert_eq!(job.next_run, Some(T0 + 30_000));

    // 1ms early: not due.
    clock.advance_ms(29_999);
    let j = store.get(&job.id).unwrap();
    assert!(j.next_run.unwrap() > store.now_ms(), "due too early");

    // At exactly +30s: due.
    clock.advance_ms(1);
    let j = store.get(&job.id).unwrap();
    assert!(
        j.next_run.unwrap() <= store.now_ms(),
        "not due at fire time"
    );
}

#[test]
fn every_5min_fires_exactly_three_times_across_15min_advance() {
    let (mut store, clock) = mock_store(T0);
    let job = store
        .create(make_params("rep", "ws", "*/5 * * * *"))
        .unwrap();

    let mut fires = 0;
    // Walk time forward to a hair past each next_run, recording a success per fire — the scheduler's decision rule (next_run <= now) verbatim.
    let deadline = T0 + 15 * 60_000 + 1_000; // T0 + 15min (+1s slack)
    loop {
        let next = store.get(&job.id).unwrap().next_run.unwrap();
        if next > deadline {
            break;
        }
        clock.set_ms(next);
        assert!(next <= store.now_ms(), "fire decision must be due now");
        fires += 1;
        store
            .record_run(
                &job.id,
                run(&format!("r{}", fires), next, CronRunStatus::Success),
            )
            .unwrap();
        let after = store.get(&job.id).unwrap().next_run.unwrap();
        assert!(after > next, "next_run must move strictly forward");
    }
    // 22:15, 22:20, 22:25 — exactly three boundaries within T0+15min.
    assert_eq!(fires, 3, "expected exactly 3 fires in 15 minutes");
}

#[test]
fn failure_backoff_is_anchored_to_mock_now() {
    let (mut store, clock) = mock_store(T0);
    let job = store
        .create(make_params("backoff", "ws", "0 * * * *"))
        .unwrap();
    clock.advance_ms(60_000);
    let now = store.now_ms();
    store
        .record_run(&job.id, run("r1", now, CronRunStatus::Failed))
        .unwrap();
    let next = store.get(&job.id).unwrap().next_run.unwrap();
    assert!(next > now, "backoff must schedule in the mocked future");
    assert!(
        next - now <= 3_600_000 + 3_600_000 / 4 + 1,
        "backoff must respect the 1h cap (+25% jitter)"
    );
}

#[test]
fn update_run_status_stamps_finished_at_from_mock_clock() {
    let (mut store, clock) = mock_store(T0);
    let job = store
        .create(make_params("stamp", "ws", "0 * * * *"))
        .unwrap();
    store
        .record_run(&job.id, run("r1", T0, CronRunStatus::Running))
        .unwrap();
    clock.set_ms(T0 + 123_456);
    store
        .update_run_status(&job.id, "r1", CronRunStatus::Success)
        .unwrap();
    let history = store.get_history(&job.id);
    assert_eq!(history[0].finished_at, Some(T0 + 123_456));
}
