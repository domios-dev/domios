use super::*;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn role_launch_identity_reaches_the_spawned_agent() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let _storage = fixture.install_seeded_storage_desk();
    let svc = Arc::new(fixture.service.clone());
    let clock = codeterm_cron::clock::MockClock::starting_at(1_800_000_000_000);
    let cron: CronState = Arc::new(parking_lot::Mutex::new(
        super::super::store::CronStore::new_with_path_and_clock(
            fixture.temp.path().join("cron.json"),
            clock.clone(),
        ),
    ));
    assert!(svc.set_ext(cron.clone()).is_ok());
    let workspace = svc.mux.read().list_workspaces()[0].id.clone();
    let job = cron
        .lock()
        .create(
            serde_json::from_value(serde_json::json!({
                "name":"disposable", "workspace_id":workspace, "schedule":"in 1h",
                "task":"opaque-fixture", "agent":"mirror:fixture",
                "cwd":fixture.temp.path(), "enabled":true, "oneshot":true, "jitter_max_ms":0
            }))
            .unwrap(),
        )
        .unwrap();
    clock.set_ms(job.next_run.unwrap());
    let resolved = role_launch_spec(
        &fire_spec(&job),
        crate::service::role_policy::CronLaunch {
            provider: "portable-cron-fixture".into(),
            launch_profile: Some("opaque-profile".into()),
            model: "opaque-model".into(),
            effort: Some("opaque-effort".into()),
            task: "opaque-digest".into(),
        },
    );
    let spawned = std::sync::Mutex::new(None);
    let outcome = fire_job_with_spawn(&cron, &svc, &resolved, |spec| {
        let launch = spec.chatbackend_launch.as_ref().unwrap();
        *spawned.lock().unwrap() = Some((
            launch.params.agent.clone(),
            launch.params.launch_profile.clone(),
            launch.params.model.clone(),
            launch.params.reasoning_effort.clone(),
        ));
        let tab_id = svc.mux.write().create_tab(&workspace)?;
        Ok(crate::service::spawn::TabHandle {
            command_submission: None,
            tab_id,
            pty_id: None,
            workspace_id: workspace.clone(),
            description: None,
            worker_id: None,
            warning: None,
            model_fallback: None,
            budget_notice: None,
        })
    });
    assert!(matches!(outcome, FireStart::Recorded));
    assert_eq!(
        spawned.into_inner().unwrap().unwrap(),
        (
            Some("portable-cron-fixture".to_string()),
            Some("opaque-profile".to_string()),
            Some("opaque-model".to_string()),
            Some("opaque-effort".to_string()),
        )
    );
    crate::service::session_persist::stop_worker(&svc).unwrap();
}

#[test]
fn process_split_contract_manual_and_scheduled_cron_publish_exact_internal_run() {
    const JOB_NAME: &str = "Scout cron QA";

    for scheduled in [false, true] {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        let _storage = fixture.install_seeded_storage_desk();
        let svc = Arc::new(fixture.service.clone());
        let clock = codeterm_cron::clock::MockClock::starting_at(1_800_000_000_000);
        let cron: CronState = Arc::new(parking_lot::Mutex::new(
            super::super::store::CronStore::new_with_path_and_clock(
                fixture.temp.path().join("cron.json"),
                clock.clone(),
            ),
        ));
        assert!(svc.set_ext(cron.clone()).is_ok());
        let workspace = svc.mux.read().list_workspaces()[0].id.clone();
        let job = cron
            .lock()
            .create(
                serde_json::from_value(serde_json::json!({
                    "name":JOB_NAME, "workspace_id":workspace, "schedule":"in 1h",
                    "task":"opaque-fixture", "agent":"portable-cron-fixture", "model":"fixture",
                    "cwd":fixture.temp.path(), "enabled":scheduled, "oneshot":true, "jitter_max_ms":0
                }))
                .unwrap(),
            )
            .unwrap();
        let mut events = svc.events.subscribe();
        let spawn = |mut spec: crate::service::spawn::SpawnTabSpec| {
            crate::service::spawn::prepare_tab_identity_for_test(&mut spec)?;
            assert_eq!(spec.workspace_id.as_deref(), Some(workspace.as_str()));
            let mut mux = svc.mux.write();
            let tab_id = mux.create_tab(&workspace)?;
            spec.apply_pty_agent_identity(&mut mux, &tab_id, None);
            Ok(crate::service::spawn::TabHandle {
                command_submission: None,
                tab_id,
                pty_id: None,
                workspace_id: workspace.clone(),
                description: spec.description,
                worker_id: None,
                warning: None,
                model_fallback: None,
                budget_notice: None,
            })
        };
        let run = if scheduled {
            clock.set_ms(job.next_run.unwrap());
            assert!(matches!(
                fire_job_with_spawn(&cron, &svc, &fire_spec(&job), spawn),
                FireStart::Recorded
            ));
            (*cron.lock().get_history(&job.id)[0]).clone()
        } else {
            svc.cron_trigger_with_spawn(&cron, &job.id, spawn).unwrap()
        };
        assert_eq!(run.scheduled_occurrence_at, job.next_occurrence_at);
        assert_eq!(run.fired_at, Some(run.started_at));
        let tab_id = run.tab_id.as_deref().expect("successful fixture launch");
        assert_eq!(run.status, super::super::store::CronRunStatus::Running);
        {
            let mux = svc.mux.read();
            let tab = mux.get_tab(tab_id).expect("registered run tab");
            assert_eq!(tab.title, "Cron");
            assert_eq!(tab.description.as_deref(), Some(JOB_NAME));
        }
        let fired = crate::async_runtime::block_on(async {
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let event = events.recv().await.unwrap();
                    if event.frame.event_type == "cron-fired" {
                        break event;
                    }
                }
            })
            .await
            .unwrap()
        });
        assert!(!fired.public);
        let codeterm_events::SystemEvent::Generic { data, .. } = fired.event else {
            panic!("expected cron payload");
        };
        assert_eq!(
            data,
            crate::service::cron::cron_fired_payload(&job.id, &run.run_id, tab_id)
        );
        assert_eq!(
            fixture
                .ui
                .0
                .lock()
                .iter()
                .filter(|(name, payload)| { name == "cron-fired" && payload == &data })
                .count(),
            1
        );
        assert_eq!(cron.lock().get_history(&job.id).len(), 1);
        svc.close_tab_with_reason(tab_id, crate::multiplexer::types::TabCloseReason::Cancelled)
            .unwrap();
        crate::service::session_persist::stop_worker(&svc).unwrap();
    }
}
