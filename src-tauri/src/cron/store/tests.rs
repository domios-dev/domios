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
        schedule: schedule.to_string(),
        task: "test task".to_string(),
        description: None,
        agent: Some("claude".to_string()),
        model: None,
        cwd: None,
        role: None,
        creator_pane_id: None,
        enabled: None,
        max_history: None,
        oneshot: None,
        jitter_max_ms: Some(0),
        tz: None,
        timeout_seconds: None,
    }
}

#[test]
fn record_failure_increments_counter_and_backs_off() {
    let mut store = temp_store();
    let job = store.create(make_params("Test", "ws", "0 * * * *")).unwrap();
    let initial_next = job.next_run.unwrap();

    store.record_run(&job.id, CronRunInfo {
        run_id: "r1".into(),
        started_at: 0,
        finished_at: Some(0),
        status: CronRunStatus::Failed,
        pane_id: None,
        tab_id: None,
    }).unwrap();

    let after = store.get(&job.id).unwrap();
    assert_eq!(after.consecutive_failures, 1);
    let _ = initial_next;
}

#[test]
fn record_5_failures_auto_disables_job() {
    let mut store = temp_store();
    let job = store.create(make_params("Test", "ws", "0 * * * *")).unwrap();
    for i in 0..MAX_CONSECUTIVE_FAILURES {
        store.record_run(&job.id, CronRunInfo {
            run_id: format!("r{}", i),
            started_at: 0,
            finished_at: Some(0),
            status: CronRunStatus::Failed,
            pane_id: None,
            tab_id: None,
        }).unwrap();
    }
    let after = store.get(&job.id).unwrap();
    assert!(!after.enabled, "should auto-disable after {} failures", MAX_CONSECUTIVE_FAILURES);
    assert_eq!(after.consecutive_failures, MAX_CONSECUTIVE_FAILURES);
}

#[test]
fn success_resets_failure_counter() {
    let mut store = temp_store();
    let job = store.create(make_params("Test", "ws", "0 * * * *")).unwrap();
    for i in 0..3 {
        store.record_run(&job.id, CronRunInfo {
            run_id: format!("f{}", i),
            started_at: 0,
            finished_at: Some(0),
            status: CronRunStatus::Failed,
            pane_id: None,
            tab_id: None,
        }).unwrap();
    }
    store.record_run(&job.id, CronRunInfo {
        run_id: "ok".into(),
        started_at: 0,
        finished_at: Some(0),
        status: CronRunStatus::Success,
        pane_id: None,
        tab_id: None,
    }).unwrap();
    let after = store.get(&job.id).unwrap();
    assert_eq!(after.consecutive_failures, 0);
    assert!(after.enabled);
}

#[test]
fn test_cron_crud() {
    let mut store = temp_store();

    let job = store.create(make_params("Test Cron", "ws-1", "*/30 * * * *")).unwrap();
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
fn is_job_running_tracks_registered_panes() {
    let mut store = temp_store();
    let job = store.create(make_params("Runner", "ws-1", "0 * * * *")).unwrap();
    assert!(!store.is_job_running(&job.id));

    store.register_pane("pane-A", &job.id, "run-1");
    assert!(store.is_job_running(&job.id));

    store.register_pane("pane-B", &job.id, "run-2");
    assert!(store.is_job_running(&job.id));

    store.unregister_pane("pane-A");
    assert!(store.is_job_running(&job.id));

    store.unregister_pane("pane-B");
    assert!(!store.is_job_running(&job.id));
}

#[test]
fn test_list_for_workspace() {
    let mut store = temp_store();
    store.create(make_params("Job A", "ws-1", "0 * * * *")).unwrap();
    store.create(make_params("Job B", "ws-2", "0 * * * *")).unwrap();
    store.create(make_params("Job C", "ws-1", "0 * * * *")).unwrap();

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
                    pane_id: None,
                    tab_id: None,
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
        store.create(make_params("Persistent", "ws-1", "0 9 * * *")).unwrap();
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

    let job = store.create(CronCreateParams {
        oneshot: Some(true),
        ..make_params("Once", "ws-1", "in 10m")
    }).unwrap();

    assert!(job.oneshot);
    assert!(job.enabled);
    assert!(job.next_run.is_some());

    store.record_run(&job.id, CronRunInfo {
        run_id: "run-1".into(),
        started_at: 1000,
        finished_at: Some(2000),
        status: CronRunStatus::Success,
        pane_id: None,
        tab_id: None,
    }).unwrap();

    let j = store.get(&job.id).unwrap();
    assert!(!j.enabled);
    assert!(j.next_run.is_none());
}

#[test]
fn create_accepts_tz_and_recomputes_next_run() {
    let mut store = temp_store();
    let job_utc = store.create(make_params("UTC job", "ws", "0 3 * * *")).unwrap();
    let job_riga = store.create(CronCreateParams {
        tz: Some("Europe/Riga".into()),
        ..make_params("Riga job", "ws", "0 3 * * *")
    }).unwrap();
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
    let job = store.create(CronCreateParams {
        tz: Some("   ".into()),
        ..make_params("EmptyTz", "ws", "0 3 * * *")
    }).unwrap();
    assert_eq!(job.tz, None);
}

#[test]
fn create_accepts_timeout_seconds() {
    let mut store = temp_store();
    let none_job = store.create(make_params("NoTimeout", "ws", "0 * * * *")).unwrap();
    let with_job = store.create(CronCreateParams {
        timeout_seconds: Some(1800),
        ..make_params("HasTimeout", "ws", "0 * * * *")
    }).unwrap();
    let zeroed = store.create(CronCreateParams {
        timeout_seconds: Some(0),
        ..make_params("ZeroTimeout", "ws", "0 * * * *")
    }).unwrap();
    assert_eq!(none_job.timeout_seconds, None);
    assert_eq!(with_job.timeout_seconds, Some(1800));
    // 0 means "no override" — normalized to None on create.
    assert_eq!(zeroed.timeout_seconds, None);
}

#[test]
fn update_can_set_clear_tz_and_timeout() {
    let mut store = temp_store();
    let job = store.create(make_params("ToPatch", "ws", "0 3 * * *")).unwrap();

    let updated = store.update(&job.id, CronUpdateParams {
        name: None, description: None, schedule: None, task: None, cwd: None, role: None,
        enabled: None, max_history: None, agent: None, model: None, jitter_max_ms: None,
        tz: Some("Europe/Riga".into()),
        timeout_seconds: Some(900),
    }).unwrap();
    assert_eq!(updated.tz.as_deref(), Some("Europe/Riga"));
    assert_eq!(updated.timeout_seconds, Some(900));

    // Empty tz string clears back to UTC; timeout 0 clears override.
    let cleared = store.update(&job.id, CronUpdateParams {
        name: None, description: None, schedule: None, task: None, cwd: None, role: None,
        enabled: None, max_history: None, agent: None, model: None, jitter_max_ms: None,
        tz: Some("".into()),
        timeout_seconds: Some(0),
    }).unwrap();
    assert_eq!(cleared.tz, None);
    assert_eq!(cleared.timeout_seconds, None);
}

#[test]
fn update_rejects_invalid_tz() {
    let mut store = temp_store();
    let job = store.create(make_params("X", "ws", "0 3 * * *")).unwrap();
    let r = store.update(&job.id, CronUpdateParams {
        name: None, description: None, schedule: None, task: None, cwd: None, role: None,
        enabled: None, max_history: None, agent: None, model: None, jitter_max_ms: None,
        tz: Some("Imaginary/Place".into()),
        timeout_seconds: None,
    });
    assert!(r.is_err());
}

/// Regression for issue #97: editing only `schedule` (no tz, no jitter) must
/// recompute `next_run`, not leave it anchored to the previous schedule.
#[test]
fn update_schedule_only_recomputes_next_run() {
    let mut store = temp_store();
    let job = store.create(CronCreateParams {
        tz: Some("Europe/Riga".into()),
        jitter_max_ms: Some(0),
        ..make_params("DailyCron", "ws", "0 6 * * *")
    }).unwrap();
    let before = job.next_run.expect("created cron must have next_run");

    let updated = store.update(&job.id, CronUpdateParams {
        name: None, description: None,
        schedule: Some("0 3 * * *".into()),
        task: None, cwd: None, role: None,
        enabled: None, max_history: None, agent: None, model: None,
        jitter_max_ms: None,
        tz: None,
        timeout_seconds: None,
    }).unwrap();

    assert_eq!(updated.schedule, "0 3 * * *");
    assert_eq!(updated.tz.as_deref(), Some("Europe/Riga"));
    let after = updated.next_run.expect("next_run must remain set after edit");
    assert_ne!(
        before, after,
        "schedule-only edit must move next_run off the old schedule (was {}, still {})",
        before, after
    );
}
