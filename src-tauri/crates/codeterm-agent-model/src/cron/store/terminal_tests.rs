use super::*;

#[test]
fn terminal_completion_preserves_every_field_and_unknown_runs_are_not_fresh() {
    for status in [
        CronRunStatus::Cancelled,
        CronRunStatus::Failed,
        CronRunStatus::Success,
    ] {
        let (mut store, clock) = mock_store(T0);
        let job = store
            .create(make_params("Terminal receipt", "ws", "0 * * * *"))
            .unwrap();
        store
            .record_run(
                &job.id,
                run("owned", T0, CronRunStatus::AwaitingOwnerPrompt),
            )
            .unwrap();
        store.set_run_session(&job.id, "owned", "original-session", Some("original-cwd"));
        store
            .set_run_report(&job.id, "owned", "original-summary", "delivered".into())
            .unwrap();
        clock.set_ms(T0 + 10);
        assert!(store.update_run_status(&job.id, "owned", status).unwrap());
        let receipt = store.get_history(&job.id)[0].clone();
        let definition = store.get(&job.id).unwrap().clone();
        clock.set_ms(T0 + 1_200_000);
        let duplicate = store
            .prepare_finish_run_with_summary(
                &job.id,
                "owned",
                CronRunStatus::Failed,
                Some(RunSession {
                    session_id: "foreign",
                    cwd: Some("foreign"),
                }),
                Some("hard timeout after 1200s"),
                None,
            )
            .unwrap();
        assert!(!duplicate.value.fresh);
        assert!(duplicate.changes.is_empty());
        store.legacy(Ok(duplicate)).unwrap();
        store.set_run_session(&job.id, "owned", "late-session", None);
        store
            .set_run_report(&job.id, "owned", "late-summary", "late-report".into())
            .unwrap();
        assert_eq!(store.get_history(&job.id), vec![&receipt]);
        assert_eq!(store.get(&job.id), Some(&definition));
        assert!(
            !store
                .prepare_finish_run(&job.id, "unknown", CronRunStatus::Failed, None)
                .unwrap()
                .value
                .fresh
        );
    }
}

#[test]
fn a_cancelled_or_reported_terminal_mirror_refuses_a_late_history_report_on_reopen() {
    for cancelled in [true, false] {
        let (mut store, clock) = mock_store(T0);
        let job = store
            .create(make_params("Mirror authority", "ws", "0 * * * *"))
            .unwrap();
        store
            .record_run(&job.id, run("owned", T0, CronRunStatus::Running))
            .unwrap();
        let mirror = store
            .file
            .jobs
            .iter_mut()
            .find(|candidate| candidate.id == job.id)
            .unwrap()
            .last_run
            .as_mut()
            .unwrap();
        mirror.status = if cancelled {
            CronRunStatus::Cancelled
        } else {
            CronRunStatus::Success
        };
        mirror.cancel_reason = cancelled.then_some(CronCancelReason::UserCancel);
        mirror.finished_at = Some(T0 + 5);
        mirror.summary = Some("retained mirror summary".into());
        if !cancelled {
            mirror.report_delivery = Some("delivered".into());
        }
        store.save().unwrap();
        let path = store.backend.legacy_path().unwrap().to_path_buf();
        let original = store.get(&job.id).unwrap().clone();
        let history = store.get_history(&job.id)[0].clone();
        let prepared =
            store.prepare_set_run_report(&job.id, "owned", "late report", "late delivery");
        assert!(prepared.changes.is_empty());
        store.legacy(Ok(prepared)).unwrap();
        let reopened = CronStore::new_with_path_and_clock(path, clock);
        assert_eq!(reopened.get(&job.id), Some(&original));
        assert_eq!(reopened.get_history(&job.id), vec![&history]);
    }
}
