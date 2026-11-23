use super::*;
use crate::cron::store::CronCancelReason;

const NONCE: &str = "original-generation";

fn journal_provisional_owner(run: &OneshotRun) -> CronRunAuthority {
    let svc = &run.fixture.service;
    svc.run_watcher
        .reserve_cron_slot(svc, &run.cron, &run.job_id, &run.run_id)
        .unwrap();
    let owner = CronRunAuthority {
        tab_id: RUN_TAB.into(),
        job_id: run.job_id.clone(),
        run_id: run.run_id.clone(),
        binding_nonce: Some(NONCE.into()),
        pty_id: None,
    };
    svc.run_watcher.journal_cron_owner(&owner).unwrap();
    assert_eq!(
        svc.run_watcher
            .cron_record(&run.job_id, &run.run_id)
            .unwrap()
            .phase,
        recovery::Phase::Provisional
    );
    owner
}

fn reconcile_restart(run: &OneshotRun) -> ServiceContext {
    let svc = &run.fixture.service;
    let restored = crate::cron::RestoredCronSession::from_mux(&svc.mux.read());
    crate::cron::reconcile_restored_runs(&run.cron, &restored).unwrap();
    let mut restarted = svc.clone();
    restarted.run_watcher = Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new());
    restarted
}

fn prune_original_receipt(run: &OneshotRun) {
    let missed = CronRunInfo {
        scheduled_occurrence_at: None,
        fired_at: None,
        late_by_ms: None,
        provider: None,
        owner_instance_id: None,
        cancel_reason: None,
        run_id: "missed-occurrence".into(),
        started_at: 2,
        finished_at: Some(2),
        status: CronRunStatus::Cancelled,
        tab_id: None,
        session_id: None,
        cwd: None,
        summary: Some("Missed scheduled occurrence".into()),
        report_delivery: None,
    };
    commit(&run.cron, |store| {
        store.prepare_record_run(&run.job_id, missed.clone())
    })
    .unwrap();
    assert!(run
        .cron
        .lock()
        .run_receipt(&run.job_id, &run.run_id)
        .is_none());
}

fn assert_released_without_replay(run: &OneshotRun, svc: &ServiceContext) {
    assert!(svc
        .run_watcher
        .cron_record(&run.job_id, &run.run_id)
        .is_none());
    assert!(!svc.run_watcher.cron_job_finalizing(&run.job_id));
    assert!(!svc.run_watcher.unknown_cron_outcome(&run.run_id));
    let path = run
        .cron
        .lock()
        .log_file(&run.job_id, &run.run_id)
        .unwrap()
        .with_extension("terminal-adjudicated.json");
    let evidence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(evidence["phase"], "Adjudicated");
    assert_eq!(evidence["unknown_at"], "Provisional");
    assert_eq!(evidence["owner"]["nonce"], NONCE);
    svc.run_watcher
        .reserve_cron_slot(svc, &run.cron, &run.job_id, "next-occurrence")
        .unwrap();
}

#[test]
fn restart_releases_interrupted_provisional_owner_whose_tab_was_not_restored() {
    for (already_unknown, pruned) in [(false, false), (true, false), (true, true)] {
        let run = oneshot_run_with_history(None, Some(1), |fixture| {
            CronStore::new_with_path(fixture.temp.path().join("crons.json"))
        });
        let owner = journal_provisional_owner(&run);
        if already_unknown {
            run.fixture
                .service
                .run_watcher
                .mark_cron_unknown(&owner)
                .unwrap();
        }
        let restarted = reconcile_restart(&run);
        if pruned {
            prune_original_receipt(&run);
        } else {
            assert!(run.cron.lock().run_is_cancelled(&run.job_id, &run.run_id));
        }
        let before = run
            .cron
            .lock()
            .run_receipt(&run.job_id, &run.run_id)
            .cloned();
        restarted
            .run_watcher
            .ensure_cron_recovery(&restarted, &run.cron)
            .unwrap();
        assert_eq!(
            run.cron
                .lock()
                .run_receipt(&run.job_id, &run.run_id)
                .cloned(),
            before,
            "recovery adjudicates without writing a terminal receipt"
        );
        assert_released_without_replay(&run, &restarted);
    }
}

#[test]
fn restart_retains_live_provisional_owner_until_exact_cancel() {
    let run = oneshot_run(None);
    journal_provisional_owner(&run);
    let mut restarted = run.fixture.service.clone();
    restarted.run_watcher = Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new());
    restarted
        .run_watcher
        .ensure_cron_recovery(&restarted, &run.cron)
        .unwrap();
    assert!(run.cron.lock().run_is_active(&run.job_id, &run.run_id));
    let record = restarted
        .run_watcher
        .cron_record(&run.job_id, &run.run_id)
        .unwrap();
    assert_eq!(record.unknown_at, Some(recovery::Phase::Provisional));
    assert!(restarted.run_watcher.cron_job_finalizing(&run.job_id));
    assert!(restarted
        .run_watcher
        .reserve_cron_slot(&restarted, &run.cron, &run.job_id, "next-occurrence")
        .is_err());
    assert_eq!(
        restarted
            .cron_cancel_running(&run.cron, &run.job_id, CronCancelReason::UserCancel)
            .unwrap(),
        1
    );
    assert_eq!(
        run.cron
            .lock()
            .run_receipt(&run.job_id, &run.run_id)
            .unwrap()
            .status,
        CronRunStatus::Cancelled
    );
    assert_released_without_replay(&run, &restarted);
}

#[test]
fn exact_cancel_releases_retained_unknown_provisional_owner_without_active_run() {
    for pruned in [false, true] {
        let run = oneshot_run_with_history(None, Some(1), |fixture| {
            CronStore::new_with_path(fixture.temp.path().join("crons.json"))
        });
        let svc = &run.fixture.service;
        let owner = journal_provisional_owner(&run);
        svc.run_watcher.mark_cron_unknown(&owner).unwrap();
        commit(&run.cron, |store| {
            store.prepare_finish_run_with_summary(
                &run.job_id,
                &run.run_id,
                CronRunStatus::Cancelled,
                None,
                Some("interrupted"),
                Some(CronCancelReason::RestartReconcile),
            )
        })
        .unwrap();
        if pruned {
            prune_original_receipt(&run);
        }
        assert!(svc.run_watcher.cron_job_finalizing(&run.job_id));
        assert_eq!(
            svc.cron_cancel_running(&run.cron, &run.job_id, CronCancelReason::UserCancel)
                .unwrap(),
            1,
            "a released retained owner is reported instead of 'no running executions'"
        );
        assert_released_without_replay(&run, svc);
    }
}
