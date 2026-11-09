use super::*;

#[tokio::test]
async fn accepted_operations_publish_terminal_events_and_recoverable_job_state() {
    use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    for outcome in [Ok(()), Err("fixture failure".to_string())] {
        let expected = if outcome.is_ok() {
            SetupState::Installed
        } else {
            SetupState::Failed
        };
        let (send, receive) = tokio::sync::oneshot::channel();
        let sender = Mutex::new(Some(send));
        let listener = fixture.ui.listen(EVENT, move |payload| {
            if payload["state"] == "installed" || payload["state"] == "failed" {
                if let Some(send) = sender.lock().take() {
                    let _ = send.send(payload.clone());
                }
            }
        });
        let provider = format!("fixture-{}", uuid::Uuid::new_v4());
        let accepted = start(
            &fixture.service,
            provider.clone(),
            "registry-profile".into(),
            move || outcome,
        )
        .unwrap();
        assert_eq!(accepted.state, SetupState::Accepted);
        let terminal = tokio::time::timeout(std::time::Duration::from_secs(2), receive)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(terminal["jobId"], accepted.job_id);
        assert_eq!(
            current(&provider, "registry-profile").unwrap().state,
            expected
        );
        assert_eq!(for_provider(&provider).len(), 1);
        fixture.ui.unlisten(listener);
        forget_finished(&provider, "registry-profile");
        assert!(for_provider(&provider).is_empty());
    }
}

#[test]
fn setup_remains_active_until_terminal_completion_and_refuses_duplicate_work() {
    let mut jobs = Jobs::default();
    let job = jobs.reserve("fixture", "profile").unwrap();
    assert_eq!(job.state, SetupState::Accepted);
    assert!(jobs.reserve("fixture", "profile").is_err());
    assert_eq!(
        jobs.advance(&job, SetupState::Installing, None)
            .unwrap()
            .state,
        SetupState::Installing
    );
    assert_eq!(
        jobs.advance(&job, SetupState::Installed, None)
            .unwrap()
            .state,
        SetupState::Installed
    );
    let replacement = jobs.reserve("fixture", "profile").unwrap();
    assert_ne!(replacement.job_id, job.job_id);
    assert!(jobs
        .advance(&job, SetupState::Failed, Some("stale".into()))
        .is_none());
    assert_eq!(
        jobs.advance(
            &replacement,
            SetupState::Failed,
            Some("fixture error".into())
        )
        .unwrap()
        .error
        .as_deref(),
        Some("fixture error")
    );
}

#[test]
fn installer_capacity_is_bounded_and_terminal_work_frees_capacity() {
    let mut jobs = Jobs::default();
    let active: Vec<_> = (0..MAX_ACTIVE)
        .map(|id| jobs.reserve("fixture", &id.to_string()).unwrap())
        .collect();
    assert!(jobs.reserve("fixture", "extra").is_err());
    jobs.advance(&active[0], SetupState::Installed, None)
        .unwrap();
    assert!(jobs.reserve("fixture", "extra").is_ok());
}

#[test]
fn retained_jobs_evict_only_completed_work_at_the_storage_bound() {
    let mut jobs = Jobs::default();
    let active = jobs.reserve("fixture", "active").unwrap();
    for id in 0..MAX_JOBS {
        let job = jobs.reserve("fixture", &id.to_string()).unwrap();
        jobs.advance(&job, SetupState::Installed, None).unwrap();
    }
    assert_eq!(jobs.0.len(), MAX_JOBS);
    assert_eq!(
        jobs.0[&("fixture".into(), "active".into())].job_id,
        active.job_id
    );
}
