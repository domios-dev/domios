use super::*;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn process_split_contract_manual_and_scheduled_cron_publish_exact_internal_run() {
    for scheduled in [false, true] {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        let _storage = fixture.install_seeded_storage_desk();
        let svc = Arc::new(fixture.service.clone());
        let cron: CronState = Arc::new(parking_lot::Mutex::new(
            super::super::store::CronStore::new_with_path(fixture.temp.path().join("cron.json")),
        ));
        assert!(svc.set_ext(cron.clone()).is_ok());
        let workspace = svc.mux.read().list_workspaces()[0].id.clone();
        let job = cron
            .lock()
            .create(
                serde_json::from_value(serde_json::json!({
                    "name":"disposable", "workspace_id":workspace, "schedule":"in 1h",
                    "task":"opaque-fixture", "agent":"portable-cron-fixture", "model":"fixture",
                    "cwd":fixture.temp.path(), "enabled":false, "oneshot":true, "jitter_max_ms":0
                }))
                .unwrap(),
            )
            .unwrap();
        let mut events = svc.events.subscribe();
        let spawn = |spec: crate::service::spawn::SpawnTabSpec| {
            assert_eq!(spec.workspace_id.as_deref(), Some(workspace.as_str()));
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
        };
        let run = if scheduled {
            assert!(matches!(
                fire_job_with_spawn(&cron, &svc, &fire_spec(&job), spawn),
                FireStart::Recorded
            ));
            (*cron.lock().get_history(&job.id)[0]).clone()
        } else {
            svc.cron_trigger_with_spawn(&cron, &job.id, spawn).unwrap()
        };
        let tab_id = run.tab_id.as_deref().expect("successful fixture launch");
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
