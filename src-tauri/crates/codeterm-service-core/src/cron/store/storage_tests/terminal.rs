use super::*;
use crate::cron::store::CronCancelReason;

#[test]
fn storage_cron_cancelled_receipt_survives_stale_timeout_and_next_occurrence() {
    let f = fixture();
    let job = f.create(params("Cancelled receipt"));
    f.start(&job.id, "owned");
    f.cron.lock().register_tab("tab-owned", &job.id, "owned");
    commit(&f.cron, |store| Ok(store.prepare_set_run_session(&job.id, "owned", RunSession {
        session_id: "original-session", cwd: Some("original-cwd"),
    }))).unwrap();
    commit(&f.cron, |store| Ok(store.prepare_set_run_report(&job.id, "owned", "original-summary", "delivered"))).unwrap();
    commit(&f.cron, |store| Ok(store.prepare_owner_prompt("tab-owned", true))).unwrap();
    f.clock.set_ms(START_MS + 10);
    let cancelled = commit(&f.cron, |store| store.prepare_cancel_running(&job.id, store.now_ms(), CronCancelReason::UserCancel)).unwrap();
    assert_eq!(cancelled.value, vec![("owned".into(), 10)]);
    let original = f.cron.lock().get_history(&job.id)[0].clone();
    let definition = f.cron.lock().get(&job.id).unwrap().clone();
    f.clock.advance_ms(1_200_000);
    let late = commit(&f.cron, |store| store.prepare_finish_run_with_summary(
        &job.id, "owned", CronRunStatus::Failed,
        Some(RunSession { session_id: "late-session", cwd: Some("late-cwd") }),
        Some("hard timeout after 1200s"), None,
    )).unwrap();
    assert!(!late.value.fresh);
    assert!(late.receipt.is_none());
    commit(&f.cron, |store| Ok(store.prepare_set_run_report(&job.id, "owned", "late-summary", "late-report"))).unwrap();
    f.assert_cache_is_committed();
    let fresh = f.rehydrated();
    assert_eq!(fresh.get_history(&job.id), vec![&original]);
    assert_eq!(fresh.get(&job.id), Some(&definition));
    assert_eq!(original.status, CronRunStatus::Cancelled);
    assert_eq!(original.cancel_reason, Some(CronCancelReason::UserCancel));
    assert_eq!(original.finished_at, Some(START_MS + 10));
    assert_eq!(definition.consecutive_failures, 0);
    f.cron.lock().unregister_tab("tab-owned");
    let occurrence = definition.occurrence().unwrap();
    f.clock.set_ms(occurrence.due_at);
    let mut next = running("future");
    next.fired_at = Some(occurrence.due_at);
    let admitted = commit(&f.cron, |store| store.prepare_scheduled_run(&job.id, occurrence, next.clone())).unwrap();
    assert!(admitted.value.is_some());
    f.cron.lock().register_tab("future-tab", &job.id, "future");
    assert_eq!(f.cron.lock().get_tab_owner("future-tab"), Some(&(job.id.clone(), "future".into())));
    assert_eq!(f.rehydrated().get_history(&job.id)[0], &original);
}
