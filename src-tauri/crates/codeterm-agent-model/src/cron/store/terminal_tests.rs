use super::*;

#[test]
fn terminal_completion_preserves_every_field_and_unknown_runs_are_not_fresh() {
    for status in [CronRunStatus::Cancelled, CronRunStatus::Failed, CronRunStatus::Success] {
        let (mut store, clock) = mock_store(T0);
        let job = store.create(make_params("Terminal receipt", "ws", "0 * * * *")).unwrap();
        store.record_run(&job.id, run("owned", T0, CronRunStatus::AwaitingOwnerPrompt)).unwrap();
        store.set_run_session(&job.id, "owned", "original-session", Some("original-cwd"));
        store.set_run_report(&job.id, "owned", "original-summary", "delivered".into()).unwrap();
        clock.set_ms(T0 + 10);
        assert!(store.update_run_status(&job.id, "owned", status).unwrap());
        let receipt = store.get_history(&job.id)[0].clone();
        let definition = store.get(&job.id).unwrap().clone();
        clock.set_ms(T0 + 1_200_000);
        let duplicate = store.prepare_finish_run_with_summary(
            &job.id, "owned", CronRunStatus::Failed,
            Some(RunSession { session_id: "foreign", cwd: Some("foreign") }),
            Some("hard timeout after 1200s"), None,
        ).unwrap();
        assert!(!duplicate.value.fresh);
        assert!(duplicate.changes.is_empty());
        store.legacy(Ok(duplicate)).unwrap();
        store.set_run_session(&job.id, "owned", "late-session", None);
        store.set_run_report(&job.id, "owned", "late-summary", "late-report".into()).unwrap();
        assert_eq!(store.get_history(&job.id), vec![&receipt]);
        assert_eq!(store.get(&job.id), Some(&definition));
        assert!(!store.prepare_finish_run(&job.id, "unknown", CronRunStatus::Failed, None).unwrap().value.fresh);
    }
}
