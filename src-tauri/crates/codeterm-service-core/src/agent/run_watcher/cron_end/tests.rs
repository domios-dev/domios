use std::sync::Arc;

use parking_lot::Mutex;

use super::*;
use crate::cron::store::{commit, CronCreateParams, CronRunInfo, CronStorageOptions, CronStore};
use crate::service::test_fixture::ServiceFixture;

mod restart;

const RUN_TAB: &str = "cron-run-tab";
const REPORT_TAB: &str = "cron-report-tab";

type Deliveries = Arc<Mutex<Vec<(String, String)>>>;

struct OneshotRun {
    fixture: ServiceFixture,
    cron: crate::CronState,
    job_id: String,
    run_id: String,
}

fn oneshot_run(report_target: Option<&str>) -> OneshotRun {
    oneshot_run_in(report_target, |fixture| {
        CronStore::new_with_path(fixture.temp.path().join("crons.json"))
    })
}

fn oneshot_run_in(
    report_target: Option<&str>,
    store: impl FnOnce(&ServiceFixture) -> CronStore,
) -> OneshotRun {
    oneshot_run_with_history(report_target, None, store)
}

fn oneshot_run_with_history(
    report_target: Option<&str>,
    max_history: Option<usize>,
    store: impl FnOnce(&ServiceFixture) -> CronStore,
) -> OneshotRun {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let cron: crate::CronState = Arc::new(Mutex::new(store(&fixture)));
    assert!(fixture.service.set_ext(cron.clone()).is_ok());
    assert!(fixture.service.set_ext(crate::CronWake::default()).is_ok());
    let run_id = "oneshot-run".to_string();
    let params = CronCreateParams {
        name: "once".into(),
        workspace_id: "workspace".into(),
        team_id: None,
        detach_from_team: false,
        schedule: "in 1h".into(),
        task: "run once".into(),
        description: None,
        agent: None,
        model: None,
        reasoning_effort: None,
        cwd: None,
        role: None,
        skill: None,
        creator_tab_id: None,
        report_target_tab_id: report_target.map(str::to_string),
        enabled: Some(true),
        max_history,
        oneshot: Some(true),
        jitter_max_ms: Some(0),
        tz: None,
        timeout_seconds: None,
    };
    let job_id = commit(&cron, |store| store.prepare_create(params.clone()))
        .unwrap()
        .value
        .id;
    let run = CronRunInfo {
        scheduled_occurrence_at: None,
        fired_at: None,
        late_by_ms: None,
        provider: None,
        owner_instance_id: None,
        cancel_reason: None,
        run_id: run_id.clone(),
        started_at: 1,
        finished_at: None,
        status: CronRunStatus::Running,
        tab_id: Some(RUN_TAB.into()),
        session_id: None,
        cwd: None,
        summary: None,
        report_delivery: None,
    };
    commit(&cron, |store| {
        store.prepare_record_run(&job_id, run.clone())
    })
    .unwrap();
    OneshotRun {
        fixture,
        cron,
        job_id,
        run_id,
    }
}

fn end_run(run: &OneshotRun, success: bool, summary: &str, deliveries: &Deliveries) {
    let recorded = deliveries.clone();
    finish_with(
        &run.fixture.service,
        EndedCronRun {
            tab_id: RUN_TAB,
            job_id: &run.job_id,
            run_id: &run.run_id,
            success,
            summary: summary.to_string(),
            raw_output: "",
            session_id: None,
            cwd: None,
            binding_nonce: None,
            pty_id: None,
            failed_start_epoch: None,
        },
        move |tab_id, envelope, text| {
            recorded.lock().push((
                tab_id.to_string(),
                crate::service::messenger::Messenger::from(envelope).wrap(text),
            ));
            Ok(())
        },
    )
    .expect("a managed cron store settles the run")
    .join()
    .unwrap();
}

fn assert_retired_with_report(
    run: &OneshotRun,
    deliveries: &Deliveries,
    status: CronRunStatus,
    summary: &str,
) {
    let store = run.cron.lock();
    assert!(store.get(&run.job_id).is_none(), "oneshot job is retired");
    let history = store.get_history(&run.job_id);
    assert_eq!(history.len(), 1, "the ended run stays in history");
    assert_eq!(history[0].run_id, run.run_id);
    assert_eq!(history[0].status, status);
    assert_eq!(history[0].summary.as_deref(), Some(summary));
    assert_eq!(history[0].report_delivery.as_deref(), Some("delivered"));
    let log = store
        .read_log(&run.job_id, &run.run_id)
        .expect("ended run log remains available");
    assert!(log.contains(summary));

    let deliveries = deliveries.lock();
    assert_eq!(deliveries.len(), 1, "exactly one status report");
    let (target, text) = &deliveries[0];
    assert_eq!(target, REPORT_TAB);
    let context = format!("cron:once:{}", format!("{status:?}").to_lowercase());
    assert!(
        text.starts_with(&format!("<domios from=\"system\" context=\"{context}\">")),
        "{text}"
    );
    assert!(text.contains(&run.run_id), "{text}");
    assert!(text.contains(&format!("{status:?}")), "{text}");
    assert!(text.contains(summary), "{text}");
    drop(deliveries);
    drop(store);
    let visible_history = run
        .fixture
        .service
        .cron_history(&run.cron, &run.job_id)
        .expect("deleted oneshot history remains available");
    assert_eq!(visible_history.len(), 1);
    assert_eq!(visible_history[0].status, status);
}

#[test]
fn cron_failed_oneshot_keeps_history() {
    let run = oneshot_run(Some(REPORT_TAB));
    let deliveries = Deliveries::default();
    let summary = "timeout-summary-token";

    end_run(&run, false, summary, &deliveries);

    assert_retired_with_report(&run, &deliveries, CronRunStatus::Failed, summary);
    let original = run.cron.lock().get_history(&run.job_id)[0].clone();
    end_run(&run, true, "late success", &deliveries);
    assert_eq!(run.cron.lock().get_history(&run.job_id), vec![&original]);
    assert_eq!(deliveries.lock().len(), 1);
}

#[test]
fn oneshot_stop_hook_end_keeps_history_and_reports() {
    let run = oneshot_run(Some(REPORT_TAB));
    let deliveries = Deliveries::default();
    let summary = "stop-hook-summary-token";

    end_run(&run, true, summary, &deliveries);

    assert_retired_with_report(&run, &deliveries, CronRunStatus::Success, summary);
}

#[test]
fn cancelled_run_rejects_stale_timeout_without_report_or_oneshot_retirement() {
    let run = oneshot_run(Some(REPORT_TAB));
    commit(&run.cron, |store| {
        store.prepare_cancel_running(
            &run.job_id,
            123,
            crate::cron::store::CronCancelReason::UserCancel,
        )
    })
    .unwrap();
    let original = run.cron.lock().get_history(&run.job_id)[0].clone();
    let definition = run.cron.lock().get(&run.job_id).unwrap().clone();
    let deliveries = Deliveries::default();
    end_run(&run, false, "hard timeout after 1200s", &deliveries);
    assert!(deliveries.lock().is_empty());
    assert_eq!(run.cron.lock().get_history(&run.job_id), vec![&original]);
    assert_eq!(run.cron.lock().get(&run.job_id), Some(&definition));
    assert!(run.cron.lock().read_log(&run.job_id, &run.run_id).is_none());
}

#[test]
fn oneshot_end_without_report_target_keeps_history() {
    let run = oneshot_run(None);
    let deliveries = Deliveries::default();

    end_run(&run, false, "no-target-summary", &deliveries);

    assert!(deliveries.lock().is_empty());
    let store = run.cron.lock();
    assert!(store.get(&run.job_id).is_none());
    let history = store.get_history(&run.job_id);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, CronRunStatus::Failed);
    assert_eq!(
        history[0].report_delivery.as_deref(),
        Some("no report target")
    );
}

#[test]
fn storage_cron_consumer_run_end_commits_through_storage() {
    let opened = std::sync::OnceLock::new();
    let run = oneshot_run_in(Some(REPORT_TAB), |fixture| {
        crate::storage::test_support::seed_empty_root(&fixture.temp.path().join("instance"));
        let storage = crate::storage::Storage::open(
            fixture.temp.path().join("instance"),
            crate::storage::StorageConfig::default(),
        )
        .unwrap();
        let store = CronStore::open_storage(
            &storage,
            fixture.temp.path().join("crons"),
            crate::cron::clock::system_clock(),
            CronStorageOptions::default(),
            crate::config::schema::AppConfig::default()
                .general_agent
                .agent_provider,
        )
        .unwrap();
        let _ = opened.set(storage);
        store
    });
    let deliveries = Deliveries::default();
    let summary = "storage-summary-token";

    end_run(&run, false, summary, &deliveries);

    assert_retired_with_report(&run, &deliveries, CronRunStatus::Failed, summary);
    let fresh = CronStore::open_storage(
        opened.get().unwrap(),
        run.fixture.temp.path().join("crons"),
        crate::cron::clock::system_clock(),
        CronStorageOptions::default(),
        crate::config::schema::AppConfig::default()
            .general_agent
            .agent_provider,
    )
    .unwrap();
    let cache = run.cron.lock();
    assert!(fresh.get(&run.job_id).is_none());
    assert_eq!(
        fresh.get_history(&run.job_id),
        cache.get_history(&run.job_id)
    );
}

#[test]
fn cron_settle_async_runtime_progresses_while_cron_writer_is_held() {
    let run = oneshot_run(None);
    let writer = run.cron.lock();
    let (ready, progressed) = std::sync::mpsc::channel();
    let svc = run.fixture.service.clone();
    let job_id = run.job_id.clone();
    let run_id = run.run_id.clone();
    let caller = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let settlement = finish_with(
                    &svc,
                    EndedCronRun {
                        tab_id: RUN_TAB,
                        job_id: &job_id,
                        run_id: &run_id,
                        success: true,
                        summary: "held-writer-summary".into(),
                        raw_output: "",
                        session_id: None,
                        cwd: None,
                        binding_nonce: None,
                        pty_id: None,
                        failed_start_epoch: None,
                    },
                    |_, _, _| Ok(()),
                )
                .expect("managed cron state starts settlement");
                tokio::task::yield_now().await;
                ready.send(settlement).unwrap();
            });
    });

    let settlement = progressed
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("the async caller progresses while the cron writer is held");
    drop(writer);
    caller.join().unwrap();
    settlement.join().unwrap();

    let cron = run.cron.lock();
    let history = cron.get_history(&run.job_id);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, CronRunStatus::Success);
}

#[test]
fn report_persistence_refusal_keeps_delivery_continuation_without_replay() {
    let opened = std::sync::OnceLock::new();
    let run = oneshot_run_in(Some(REPORT_TAB), |fixture| {
        let root = fixture.temp.path().join("instance");
        crate::storage::test_support::seed_empty_root(&root);
        let storage =
            crate::storage::Storage::open(root, crate::storage::StorageConfig::default()).unwrap();
        let store = CronStore::open_storage(
            &storage,
            fixture.temp.path().join("crons"),
            crate::cron::clock::system_clock(),
            CronStorageOptions::default(),
            "fixture".into(),
        )
        .unwrap();
        opened.set(storage).ok().unwrap();
        store
    });
    let deliveries = Deliveries::default();
    let recorded = deliveries.clone();
    let storage = opened.get().unwrap().clone();
    finish_with(
        &run.fixture.service,
        EndedCronRun {
            tab_id: RUN_TAB,
            job_id: &run.job_id,
            run_id: &run.run_id,
            success: false,
            summary: "retained report".into(),
            raw_output: "",
            session_id: None,
            cwd: None,
            binding_nonce: None,
            pty_id: None,
            failed_start_epoch: None,
        },
        move |tab, _, text| {
            recorded.lock().push((tab.into(), text.into()));
            storage
                .inject_fault(crate::storage::Fault::FailNextCommit)
                .wait(std::time::Duration::from_secs(5))
                .unwrap();
            Ok(())
        },
    )
    .unwrap()
    .join()
    .unwrap();
    assert_eq!(deliveries.lock().len(), 1);
    assert!(run
        .fixture
        .service
        .run_watcher
        .cron_job_finalizing(&run.job_id));
    assert!(
        run.cron.lock().get(&run.job_id).is_some(),
        "oneshot retirement waits for report persistence"
    );
    assert!(run
        .cron
        .lock()
        .run_receipt(&run.job_id, &run.run_id)
        .unwrap()
        .report_delivery
        .is_none());
    let (sent, done) = std::sync::mpsc::channel();
    run.fixture.ui.listen("cron-run-finished", move |_| {
        let _ = sent.send(());
    });
    assert!(run
        .fixture
        .service
        .run_watcher
        .resume_cron_finalizer_for_run(&run.fixture.service, &run.run_id, None, None, true)
        .is_some());
    done.recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert_eq!(deliveries.lock().len(), 1, "resume must not deliver again");
    let fresh = CronStore::open_storage(
        opened.get().unwrap(),
        run.fixture.temp.path().join("crons"),
        crate::cron::clock::system_clock(),
        CronStorageOptions::default(),
        "fixture".into(),
    )
    .unwrap();
    assert!(fresh.get(&run.job_id).is_none());
    let receipt = fresh.run_receipt(&run.job_id, &run.run_id).unwrap();
    assert_eq!(receipt.summary.as_deref(), Some("retained report"));
    assert_eq!(receipt.report_delivery.as_deref(), Some("delivered"));
    assert!(run
        .fixture
        .service
        .run_watcher
        .cron_finalizers
        .lock()
        .is_empty());
    assert!(!run
        .fixture
        .service
        .run_watcher
        .cron_job_finalizing(&run.job_id));
}

#[test]
fn cron_recovery_budget_refuses_before_allocating_or_consuming_an_occurrence() {
    let run = oneshot_run(None);
    let svc = &run.fixture.service;
    let mut identities = Vec::new();
    for index in 0..=recovery::MAX_OWNERS {
        let params: CronCreateParams = serde_json::from_value(serde_json::json!({
            "name": format!("Budget {index}"), "workspace_id":"workspace",
            "schedule":"0 * * * *", "task":"opaque", "jitter_max_ms":0,
        }))
        .unwrap();
        let job = commit(&run.cron, |store| store.prepare_create(params.clone()))
            .unwrap()
            .value;
        identities.push((job.id, format!("budget-{index}")));
    }
    let tabs = svc.mux.read().snapshot().tabs.len();
    for (job, occurrence) in identities.iter().take(recovery::MAX_OWNERS) {
        svc.run_watcher
            .reserve_cron_slot(svc, &run.cron, job, occurrence)
            .unwrap();
    }
    let (job, occurrence) = identities.last().unwrap();
    let before = run.cron.lock().get(job).unwrap().clone();
    assert!(svc
        .run_watcher
        .reserve_cron_slot(svc, &run.cron, job, occurrence)
        .unwrap_err()
        .contains("capacity"));
    assert_eq!(svc.mux.read().snapshot().tabs.len(), tabs);
    assert_eq!(run.cron.lock().get(job), Some(&before));
    assert!(run.cron.lock().run_receipt(job, occurrence).is_none());
    let (resolved, old) = &identities[0];
    assert_eq!(
        svc.cron_cancel_running(
            &run.cron,
            resolved,
            crate::cron::store::CronCancelReason::UserCancel
        )
        .unwrap(),
        0
    );
    assert!(svc.run_watcher.cron_record(resolved, old).is_none());
    svc.run_watcher
        .reserve_cron_slot(svc, &run.cron, job, occurrence)
        .unwrap();
}

#[test]
fn cron_restart_pending_finish_and_delivery_intent_remain_unknown_until_exact_cancel() {
    for phase in [
        recovery::Phase::FinishPending,
        recovery::Phase::DeliveryIntent,
    ] {
        let run = oneshot_run(Some(REPORT_TAB));
        let svc = &run.fixture.service;
        svc.run_watcher
            .reserve_cron_slot(svc, &run.cron, &run.job_id, &run.run_id)
            .unwrap();
        let owner = CronRunAuthority {
            tab_id: RUN_TAB.into(),
            job_id: run.job_id.clone(),
            run_id: run.run_id.clone(),
            binding_nonce: None,
            pty_id: None,
        };
        // The crash is injected at the persisted boundary, before any delivery invocation.
        svc.run_watcher
            .checkpoint_cron(
                &run.cron,
                &owner,
                recovery::Phase::FinishPending,
                (Some(0), ""),
                None,
                None,
            )
            .unwrap();
        commit(&run.cron, |store| {
            store.prepare_finish_run_with_summary(
                &run.job_id,
                &run.run_id,
                CronRunStatus::Success,
                None,
                Some("original terminal"),
                None,
            )
        })
        .unwrap();
        if phase == recovery::Phase::DeliveryIntent {
            svc.run_watcher
                .checkpoint_cron(
                    &run.cron,
                    &owner,
                    phase,
                    (Some(0), ""),
                    Some(svc.cron_report_plan(&run.cron, &run.job_id)),
                    None,
                )
                .unwrap();
        }
        let original = run
            .cron
            .lock()
            .run_receipt(&run.job_id, &run.run_id)
            .unwrap()
            .clone();
        let mut restarted = svc.clone();
        restarted.run_watcher = Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new());
        restarted
            .run_watcher
            .ensure_cron_recovery(&restarted, &run.cron)
            .unwrap();
        assert!(restarted.run_watcher.unknown_cron_outcome(&run.run_id));
        assert_eq!(
            restarted
                .run_watcher
                .cron_record(&run.job_id, &run.run_id)
                .unwrap()
                .unknown_at,
            Some(phase)
        );
        assert!(restarted.run_watcher.cron_job_finalizing(&run.job_id));
        assert_eq!(
            run.cron.lock().run_receipt(&run.job_id, &run.run_id),
            Some(&original)
        );
        assert_eq!(
            restarted
                .cron_cancel_running(
                    &run.cron,
                    &run.job_id,
                    crate::cron::store::CronCancelReason::UserCancel
                )
                .unwrap(),
            1
        );
        assert_eq!(
            run.cron.lock().run_receipt(&run.job_id, &run.run_id),
            Some(&original)
        );
        assert!(!restarted.run_watcher.cron_job_finalizing(&run.job_id));
        let path = run
            .cron
            .lock()
            .log_file(&run.job_id, &run.run_id)
            .unwrap()
            .with_extension("terminal-adjudicated.json");
        let evidence: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(evidence["phase"], "Adjudicated");
        assert_eq!(evidence["unknown_at"], serde_json::to_value(phase).unwrap());
        assert!(
            run.cron.lock().get(&run.job_id).is_some(),
            "adjudication does not fabricate oneshot retirement"
        );
    }
}

#[test]
fn cron_journal_write_refusal_preserves_live_phase_and_external_evidence() {
    let run = oneshot_run(None);
    let svc = &run.fixture.service;
    svc.run_watcher
        .reserve_cron_slot(svc, &run.cron, &run.job_id, &run.run_id)
        .unwrap();
    let path = run
        .cron
        .lock()
        .terminal_recovery_root()
        .join(".terminal-owners.json");
    let original = std::fs::read(&path).unwrap();
    let changed = b"preserved external evidence";
    std::fs::write(&path, changed).unwrap();
    let owner = CronRunAuthority {
        tab_id: RUN_TAB.into(),
        job_id: run.job_id.clone(),
        run_id: run.run_id.clone(),
        binding_nonce: None,
        pty_id: None,
    };
    assert!(svc
        .run_watcher
        .checkpoint_cron(
            &run.cron,
            &owner,
            recovery::Phase::FinishPending,
            (Some(0), ""),
            None,
            None
        )
        .is_err());
    assert_eq!(
        svc.run_watcher
            .cron_record(&run.job_id, &run.run_id)
            .unwrap()
            .phase,
        recovery::Phase::Planned
    );
    assert_eq!(std::fs::read(&path).unwrap(), changed);
    assert_ne!(original, changed);
    let mut restarted = svc.clone();
    restarted.run_watcher = Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new());
    assert!(restarted
        .run_watcher
        .ensure_cron_recovery(&restarted, &run.cron)
        .is_err());
    assert_eq!(std::fs::read(path).unwrap(), changed);
}

#[test]
fn cron_retained_payload_is_bounded_at_utf8_boundaries() {
    let text = "🙂private".repeat(8192);
    let summary = recovery::bounded(&text, recovery::MAX_SUMMARY);
    let tail = recovery::tail(&text);
    assert!(summary.len() <= recovery::MAX_SUMMARY);
    assert!(summary.contains(&text.len().to_string()));
    assert!(tail.len() <= recovery::MAX_TAIL);
    assert!(text.ends_with(&tail));
}

#[test]
fn cron_restart_known_delivery_resumes_only_idempotent_metadata_and_retirement() {
    let opened = std::sync::OnceLock::new();
    let run = oneshot_run_in(Some(REPORT_TAB), |fixture| {
        let root = fixture.temp.path().join("instance");
        crate::storage::test_support::seed_empty_root(&root);
        let storage =
            crate::storage::Storage::open(root, crate::storage::StorageConfig::default()).unwrap();
        let store = CronStore::open_storage(
            &storage,
            fixture.temp.path().join("crons"),
            crate::cron::clock::system_clock(),
            CronStorageOptions::default(),
            "fixture".into(),
        )
        .unwrap();
        opened.set(storage).ok().unwrap();
        store
    });
    let delivered = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let port = delivered.clone();
    let storage = opened.get().unwrap().clone();
    finish_with(
        &run.fixture.service,
        EndedCronRun {
            tab_id: RUN_TAB,
            job_id: &run.job_id,
            run_id: &run.run_id,
            success: false,
            summary: "original bounded terminal".into(),
            raw_output: "",
            session_id: None,
            cwd: None,
            binding_nonce: None,
            pty_id: None,
            failed_start_epoch: None,
        },
        move |_, _, _| {
            port.fetch_add(1, Ordering::AcqRel);
            storage
                .inject_fault(crate::storage::Fault::FailNextCommit)
                .wait(std::time::Duration::from_secs(5))
                .unwrap();
            Ok(())
        },
    )
    .unwrap()
    .join()
    .unwrap();
    assert_eq!(delivered.load(Ordering::Acquire), 1);
    assert_eq!(
        run.fixture
            .service
            .run_watcher
            .cron_record(&run.job_id, &run.run_id)
            .unwrap()
            .phase,
        recovery::Phase::DeliveryKnown
    );
    let original = run
        .cron
        .lock()
        .run_receipt(&run.job_id, &run.run_id)
        .unwrap()
        .clone();
    let mut restarted = run.fixture.service.clone();
    restarted.run_watcher = Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new());
    restarted
        .run_watcher
        .ensure_cron_recovery(&restarted, &run.cron)
        .unwrap();
    assert_eq!(delivered.load(Ordering::Acquire), 1);
    let receipt = run
        .cron
        .lock()
        .run_receipt(&run.job_id, &run.run_id)
        .unwrap()
        .clone();
    assert_eq!(receipt.status, original.status);
    assert_eq!(receipt.finished_at, original.finished_at);
    assert_eq!(receipt.summary, original.summary);
    assert_eq!(receipt.report_delivery.as_deref(), Some("delivered"));
    assert!(run.cron.lock().get(&run.job_id).is_none());
    assert!(restarted
        .run_watcher
        .cron_record(&run.job_id, &run.run_id)
        .is_none());
    let fresh = CronStore::open_storage(
        opened.get().unwrap(),
        run.fixture.temp.path().join("crons"),
        crate::cron::clock::system_clock(),
        CronStorageOptions::default(),
        "fixture".into(),
    )
    .unwrap();
    assert_eq!(fresh.run_receipt(&run.job_id, &run.run_id), Some(&receipt));
}

#[test]
fn cron_oversized_restart_journal_refuses_without_erasing_evidence() {
    let run = oneshot_run(None);
    let root = run.cron.lock().terminal_recovery_root();
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join(".terminal-owners.json");
    let evidence = vec![b' '; recovery::MAX_OWNERS * 32 * 1024 + 4097];
    std::fs::write(&path, &evidence).unwrap();
    assert!(run
        .fixture
        .service
        .run_watcher
        .ensure_cron_recovery(&run.fixture.service, &run.cron)
        .unwrap_err()
        .contains("bound"));
    assert_eq!(std::fs::read(path).unwrap(), evidence);
    assert_eq!(
        run.cron
            .lock()
            .run_receipt(&run.job_id, &run.run_id)
            .unwrap()
            .status,
        CronRunStatus::Running
    );
}

#[test]
fn cron_allocation_claim_consumes_one_reserved_occurrence_before_resources() {
    let run = oneshot_run(None);
    let svc = &run.fixture.service;
    svc.run_watcher
        .reserve_cron_slot(svc, &run.cron, &run.job_id, &run.run_id)
        .unwrap();
    let tabs = svc.mux.read().snapshot().tabs.len();
    let allocation =
        crate::service::spawn::cron_launch::CronAllocation::claim(svc, &run.job_id, &run.run_id)
            .unwrap();
    assert!(svc
        .run_watcher
        .claim_cron_allocation(&run.job_id, &run.run_id)
        .is_err());
    assert!(svc
        .run_watcher
        .reserve_cron_slot(svc, &run.cron, &run.job_id, &run.run_id)
        .is_err());
    assert_eq!(svc.mux.read().snapshot().tabs.len(), tabs);
    drop(allocation);
    assert_eq!(
        run.cron
            .lock()
            .run_receipt(&run.job_id, &run.run_id)
            .unwrap()
            .status,
        CronRunStatus::Failed
    );
    assert!(svc
        .run_watcher
        .cron_record(&run.job_id, &run.run_id)
        .is_none());
}

#[test]
fn cron_journal_rejects_stale_phase_without_overwriting_original_progress() {
    let run = oneshot_run(None);
    let svc = &run.fixture.service;
    svc.run_watcher
        .reserve_cron_slot(svc, &run.cron, &run.job_id, &run.run_id)
        .unwrap();
    let mut stale = svc
        .run_watcher
        .cron_record(&run.job_id, &run.run_id)
        .unwrap();
    let mut current = stale.clone();
    current.phase = recovery::Phase::FinishPending;
    svc.run_watcher.replace_cron_record(current).unwrap();
    let path = run
        .cron
        .lock()
        .terminal_recovery_root()
        .join(".terminal-owners.json");
    let evidence = std::fs::read(&path).unwrap();
    stale.phase = recovery::Phase::DeliveryIntent;
    assert!(svc.run_watcher.replace_cron_record(stale).is_err());
    assert_eq!(
        svc.run_watcher
            .cron_record(&run.job_id, &run.run_id)
            .unwrap()
            .phase,
        recovery::Phase::FinishPending
    );
    assert_eq!(std::fs::read(path).unwrap(), evidence);
}

#[test]
fn cron_uncaptured_allocation_acknowledgment_preserves_job_and_global_resource_barriers() {
    for restart in [false, true] {
        let run = oneshot_run(None);
        let svc = &run.fixture.service;
        svc.run_watcher
            .reserve_cron_slot(svc, &run.cron, &run.job_id, &run.run_id)
            .unwrap();
        svc.run_watcher
            .claim_cron_allocation(&run.job_id, &run.run_id)
            .unwrap();
        let mut owner = svc.clone();
        if restart {
            owner.run_watcher = Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new());
            owner
                .run_watcher
                .ensure_cron_recovery(&owner, &run.cron)
                .unwrap();
        } else {
            let mut interrupted = svc
                .run_watcher
                .cron_record(&run.job_id, &run.run_id)
                .unwrap();
            interrupted.unknown_at = Some(recovery::Phase::Allocating);
            interrupted.phase = recovery::Phase::Unknown;
            svc.run_watcher.replace_cron_record(interrupted).unwrap();
        }
        assert!(owner.run_watcher.unknown_cron_outcome(&run.run_id));
        assert_eq!(
            owner
                .cron_cancel_running(
                    &run.cron,
                    &run.job_id,
                    crate::cron::store::CronCancelReason::UserCancel
                )
                .unwrap(),
            0
        );
        let receipt = run
            .cron
            .lock()
            .run_receipt(&run.job_id, &run.run_id)
            .unwrap()
            .clone();
        assert_eq!(receipt.status, CronRunStatus::Cancelled);
        let record = owner
            .run_watcher
            .cron_record(&run.job_id, &run.run_id)
            .unwrap();
        assert_eq!(record.phase, recovery::Phase::Adjudicated);
        assert_eq!(record.unknown_at, Some(recovery::Phase::Allocating));
        assert!(owner.run_watcher.cron_job_finalizing(&run.job_id));
        assert!(owner
            .run_watcher
            .reserve_cron_slot(&owner, &run.cron, &run.job_id, "future-after-ack")
            .is_err());
        let params: CronCreateParams = serde_json::from_value(serde_json::json!({
            "name":"unrelated", "workspace_id":"workspace", "schedule":"in 1h", "oneshot":true, "task":"opaque", "jitter_max_ms":0,
        })).unwrap();
        let unrelated = commit(&run.cron, |store| store.prepare_create(params.clone()))
            .unwrap()
            .value;
        owner
            .run_watcher
            .reserve_cron_slot(&owner, &run.cron, &unrelated.id, "unrelated-run")
            .unwrap();
        if restart {
            assert!(
                owner
                    .run_watcher
                    .settle_unallocated_cron_claim(&owner, &run.cron, &run.job_id, &run.run_id)
                    .is_err(),
                "restart cannot reconstruct an original release witness"
            );
            let mut again = owner.clone();
            again.run_watcher = Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new());
            again
                .run_watcher
                .ensure_cron_recovery(&again, &run.cron)
                .unwrap();
            assert!(again.run_watcher.cron_job_finalizing(&run.job_id));
            assert!(again
                .run_watcher
                .reserve_cron_slot(&again, &run.cron, &run.job_id, "future-after-restart")
                .is_err());
        } else {
            let archive = run
                .cron
                .lock()
                .log_file(&run.job_id, &run.run_id)
                .unwrap()
                .with_extension("terminal-adjudicated.json");
            std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
            std::fs::write(&archive, "retained conflicting evidence").unwrap();
            assert!(owner
                .run_watcher
                .settle_unallocated_cron_claim(&owner, &run.cron, &run.job_id, &run.run_id)
                .is_err());
            assert!(owner.run_watcher.cron_job_finalizing(&run.job_id));
            assert!(owner
                .run_watcher
                .cron_allocation_pending(&run.job_id, &run.run_id));
            std::fs::remove_file(&archive).unwrap();
            owner
                .run_watcher
                .settle_unallocated_cron_claim(&owner, &run.cron, &run.job_id, &run.run_id)
                .unwrap();
            assert!(!owner.run_watcher.cron_job_finalizing(&run.job_id));
            owner
                .run_watcher
                .reserve_cron_slot(
                    &owner,
                    &run.cron,
                    &run.job_id,
                    "future-after-original-release",
                )
                .unwrap();
        }
        assert_eq!(
            run.cron.lock().run_receipt(&run.job_id, &run.run_id),
            Some(&receipt)
        );
    }
}

#[test]
fn max_history_zero_retired_oneshot_acknowledges_only_confirmed_cleanup_without_replay() {
    let opened = std::sync::OnceLock::new();
    let run = oneshot_run_with_history(Some(REPORT_TAB), Some(0), |fixture| {
        let root = fixture.temp.path().join("instance");
        crate::storage::test_support::seed_empty_root(&root);
        let storage =
            crate::storage::Storage::open(root, crate::storage::StorageConfig::default()).unwrap();
        let store = CronStore::open_storage(
            &storage,
            fixture.temp.path().join("crons"),
            crate::cron::clock::system_clock(),
            CronStorageOptions::default(),
            "fixture".into(),
        )
        .unwrap();
        opened.set(storage).ok().unwrap();
        store
    });
    let journal = run
        .cron
        .lock()
        .terminal_recovery_root()
        .join(".terminal-owners.json");
    let restore = Arc::new(Mutex::new(None));
    let saved = restore.clone();
    let path = journal.clone();
    let commits = std::sync::atomic::AtomicUsize::new(0);
    run.cron.lock().set_stage_probe(Arc::new(move |stage| {
        if stage == crate::cron::store::storage::CronStage::Committed
            && commits.fetch_add(1, Ordering::AcqRel) == 2
        {
            *saved.lock() = Some(std::fs::read(&path).unwrap());
            std::fs::write(
                &path,
                b"external evidence blocks retirement journal settlement",
            )
            .unwrap();
        }
    }));
    let deliveries = Deliveries::default();
    end_run(&run, true, "original history-free terminal", &deliveries);
    assert_eq!(deliveries.lock().len(), 1);
    assert!(run.cron.lock().get(&run.job_id).is_none());
    assert!(run
        .cron
        .lock()
        .run_receipt(&run.job_id, &run.run_id)
        .is_none());
    let retained = restore
        .lock()
        .take()
        .expect("retirement completion reached the original persistence boundary");
    std::fs::write(&journal, retained).unwrap();
    let mut restarted = run.fixture.service.clone();
    restarted.run_watcher = Arc::new(crate::agent::run_watcher::RunWatcherRegistry::new());
    restarted
        .run_watcher
        .ensure_cron_recovery(&restarted, &run.cron)
        .unwrap();
    assert!(restarted.run_watcher.unknown_cron_outcome(&run.run_id));
    assert_eq!(
        restarted
            .cron_cancel_running(
                &run.cron,
                &run.job_id,
                crate::cron::store::CronCancelReason::UserCancel
            )
            .unwrap(),
        1
    );
    assert!(restarted
        .run_watcher
        .cron_record(&run.job_id, &run.run_id)
        .is_none());
    assert_eq!(deliveries.lock().len(), 1);
    assert!(run.cron.lock().get(&run.job_id).is_none());
    assert!(run
        .cron
        .lock()
        .run_receipt(&run.job_id, &run.run_id)
        .is_none());
}
