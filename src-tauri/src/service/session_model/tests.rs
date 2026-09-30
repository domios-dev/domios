mod model_identity_tests;

mod startup_model_refresh_tests {
    use crate::service::session_model::spawn_startup_model_refresh;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn restored_model_detection_cannot_hold_the_startup_caller() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = spawn_startup_model_refresh(move || {
            started_tx.send(()).unwrap();
            release_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("startup refresh release must arrive before its test deadline");
        })
        .unwrap();

        started_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("startup refresh must start before its test deadline");
        release_tx.send(()).unwrap();
        worker.join().unwrap();
    }
}

mod model_refresh_tests {
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use crate::agent::provider::{
        AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery,
    };
    use crate::multiplexer::commands::MuxState;
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const AGENT: &str = "fixture-agent";
    const CWD: &str = "/fixture/project";

    /// Detector oracle: answers only for the session it is asked about, and can
    /// mutate the tab mid-detection to reproduce the real read/write race.
    struct SessionModelProvider {
        provider_id: String,
        models: Arc<std::sync::Mutex<HashMap<String, String>>>,
        post_boundary_models: Arc<std::sync::Mutex<HashMap<String, String>>>,
        sessions: HashSet<String>,
        during_detect: Option<Box<dyn Fn() + Send + Sync>>,
        session_detection_calls: Option<Arc<AtomicUsize>>,
    }

    impl SessionModelProvider {
        fn new(models: &[(&str, &str)]) -> Self {
            Self {
                provider_id: AGENT.into(),
                models: Arc::new(std::sync::Mutex::new(
                    models
                        .iter()
                        .map(|(sid, model)| ((*sid).to_string(), (*model).to_string()))
                        .collect(),
                )),
                post_boundary_models: Arc::new(std::sync::Mutex::new(HashMap::new())),
                sessions: models.iter().map(|(sid, _)| (*sid).to_string()).collect(),
                during_detect: None,
                session_detection_calls: None,
            }
        }

        fn counting(models: &[(&str, &str)]) -> (Self, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            let mut provider = Self::new(models);
            provider.session_detection_calls = Some(calls.clone());
            (provider, calls)
        }

        fn with_provider(
            provider_id: &str,
            session_ids: &[&str],
        ) -> (Self, Arc<std::sync::Mutex<HashMap<String, String>>>) {
            let models = Arc::new(std::sync::Mutex::new(HashMap::new()));
            let provider = Self {
                provider_id: provider_id.into(),
                models: models.clone(),
                post_boundary_models: Arc::new(std::sync::Mutex::new(HashMap::new())),
                sessions: session_ids.iter().map(|sid| (*sid).into()).collect(),
                during_detect: None,
                session_detection_calls: None,
            };
            (provider, models)
        }

        fn racing(
            models: &[(&str, &str)],
            during_detect: impl Fn() + Send + Sync + 'static,
        ) -> Self {
            Self {
                during_detect: Some(Box::new(during_detect)),
                ..Self::new(models)
            }
        }
    }

    impl AgentProvider for SessionModelProvider {
        fn id(&self) -> &str {
            &self.provider_id
        }
        fn display_name(&self) -> &str {
            &self.provider_id
        }
        fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
            self.provider_id.clone()
        }
        fn readiness_strategy(&self) -> ReadinessStrategy {
            ReadinessStrategy::FixedDelay {
                duration: Duration::ZERO,
            }
        }
        fn task_delivery(&self) -> TaskDelivery {
            TaskDelivery::PtyInput
        }
        fn detect_from_title(&self, _: &str) -> bool {
            false
        }
        fn detect_from_output(&self, _: &str) -> bool {
            false
        }
        fn session_file_path(&self, _: &str, session_id: &str) -> Option<PathBuf> {
            self.sessions
                .contains(session_id)
                .then(|| PathBuf::from(format!("{session_id}.jsonl")))
        }
        fn session_exists(&self, _: &str, session_id: &str) -> bool {
            self.sessions.contains(session_id)
        }
        fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
            None
        }
        fn build_resume_command(&self, session_id: &str, _: bool) -> Option<String> {
            Some(format!("{} --resume {session_id}", self.provider_id))
        }
        fn detect_session_model(&self, _: &str, session_id: &str) -> Option<String> {
            self.record_session_detection();
            if let Some(during_detect) = self.during_detect.as_ref() {
                during_detect();
            }
            self.models.lock().unwrap().get(session_id).cloned()
        }
        fn session_model_transcript_boundary(
            &self,
            _: &str,
            _: &str,
        ) -> Option<codeterm_types::agent::SessionModelTranscriptBoundary> {
            Some(codeterm_types::agent::SessionModelTranscriptBoundary {
                last_entry_uuid: Some("before-relaunch".into()),
                last_entry_timestamp: None,
                last_entry_type: None,
                boundary_found: true,
            })
        }
        fn detect_session_model_after_boundary(
            &self,
            _: &str,
            session_id: &str,
            boundary: &codeterm_types::agent::SessionModelTranscriptBoundary,
        ) -> Option<String> {
            (boundary.last_entry_uuid.as_deref() == Some("before-relaunch"))
                .then(|| {
                    self.post_boundary_models
                        .lock()
                        .unwrap()
                        .get(session_id)
                        .cloned()
                })
                .flatten()
        }
        fn detect_session_reasoning_effort(&self, _: &str, _: &str) -> Option<String> {
            self.record_session_detection();
            None
        }
    }

    impl SessionModelProvider {
        fn record_session_detection(&self) {
            if let Some(calls) = self.session_detection_calls.as_ref() {
                calls.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn seeded_tab(session_id: Option<&str>) -> (MultiplexerState, String) {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some(CWD.into());
        tab.agent_type = Some(AGENT.into());
        tab.agent_type_locked = true;
        tab.agent_session_id = session_id.map(str::to_string);
        (mux, tab_id)
    }

    fn session_model(mux: &MuxState, tab_id: &str) -> Option<String> {
        mux.read()
            .get_tab(tab_id)
            .and_then(|tab| tab.agent_session_model.clone())
    }

    /// The commit half: an answer for the tab's own live session reaches
    /// `agent_session_model`, normalized exactly once by the spawn policy.
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn model_badge_commits_observed_model_for_current_session() {
        let (mut mux, tab_id) = seeded_tab(Some("session-live"));
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.session_binding_nonce = Some("binding-live".into());
        tab.reported_session_binding_nonce = Some("binding-live".into());
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(SessionModelProvider::new(&[(
                "session-live",
                "claude-opus-5",
            )])));

        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);

        assert_eq!(
            session_model(&fixture.service.mux, &tab_id).as_deref(),
            Some("opus"),
            "an answer for the tab's current session must reach the tab"
        );
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(
            tab.agent_session_model_identity,
            Some(crate::multiplexer::types::SessionModelIdentity {
                session_id: "session-live".into(),
                session_binding_nonce: "binding-live".into(),
                launch_report: None,
                model_switch_turn_confirmed: false,
            })
        );
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn model_switch_uses_the_first_post_launch_turn_after_a_stale_transcript_tail() {
        let (mut mux, tab_id) = seeded_tab(Some("session-live"));
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.session_binding_nonce = Some("binding-live".into());
        tab.reported_session_binding_nonce = Some("binding-live".into());
        tab.agent_requested_model = Some("sonnet".into());
        tab.agent_provider_resolved_model = Some("sonnet".into());
        tab.pending_model_switch_observation =
            Some(crate::multiplexer::types::PendingModelSwitchObservation {
                session_id: "session-live".into(),
                launched_model: "sonnet".into(),
                transcript_boundary: codeterm_types::agent::SessionModelTranscriptBoundary {
                    last_entry_uuid: Some("before-relaunch".into()),
                    last_entry_timestamp: None,
                    last_entry_type: None,
                    boundary_found: true,
                },
            });
        let (provider, models) = SessionModelProvider::with_provider(AGENT, &["session-live"]);
        models
            .lock()
            .unwrap()
            .insert("session-live".into(), "haiku".into());
        let post_boundary_models = provider.post_boundary_models.clone();
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(provider));

        fixture
            .service
            .record_launch_reported_model(&tab_id, AGENT, "session-live", "haiku");

        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);
        assert_eq!(session_model(&fixture.service.mux, &tab_id), None);
        assert!(fixture.service.notif.lock().list(false).is_empty());

        post_boundary_models
            .lock()
            .unwrap()
            .insert("session-live".into(), "claude-sonnet-5-5".into());
        assert!(fixture.service.observe_post_launch_turn_after_stop(&tab_id));
        let reached_new_model = crate::service::boot_wait::wait_until(
            std::time::Instant::now() + Duration::from_secs(2),
            || session_model(&fixture.service.mux, &tab_id).as_deref() == Some("sonnet"),
        );

        assert!(reached_new_model);
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_session_model.as_deref(), Some("sonnet"));
        assert!(tab.model_identity_mismatch.is_none());
        assert!(tab.model_fallback.is_none());
        assert!(tab.pending_model_switch_observation.is_none());
        drop(mux);
        assert!(fixture.service.notif.lock().list(false).is_empty());
    }

    #[cfg_attr(
        not(windows),
        ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
    )]
    #[test]
    fn model_switch_observation_survives_same_session_exit_and_claim() {
        let (mut mux, tab_id) = seeded_tab(Some("session-live"));
        let observation = crate::multiplexer::types::PendingModelSwitchObservation {
            session_id: "session-live".into(),
            launched_model: "sonnet".into(),
            transcript_boundary: codeterm_types::agent::SessionModelTranscriptBoundary {
                last_entry_uuid: Some("before-relaunch".into()),
                last_entry_timestamp: None,
                last_entry_type: None,
                boundary_found: true,
            },
        };
        {
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.pty_id = Some("pty-switch".into());
            tab.session_binding_nonce = Some("binding-switch".into());
            tab.reported_session_binding_nonce = Some("binding-switch".into());
            tab.agent_requested_model = Some("sonnet".into());
            tab.agent_provider_resolved_model = Some("sonnet".into());
            tab.pending_model_switch_observation = Some(observation.clone());
        }
        let (provider, models) = SessionModelProvider::with_provider(AGENT, &["session-live"]);
        models
            .lock()
            .unwrap()
            .insert("session-live".into(), "haiku".into());
        let post_boundary_models = provider.post_boundary_models.clone();
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(provider));

        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab));
            assert_eq!(
                tab.pending_model_switch_observation.as_ref(),
                Some(&observation)
            );
        }
        fixture
            .service
            .notify_surviving_shell_restart_required(&tab_id, "pty-switch");
        assert!(fixture.service.notif.lock().list(false).is_empty());

        let claim = crate::api::hook_routes::bind_reported_session(
            &fixture.service.mux,
            fixture.service.agent_registry.as_ref(),
            &tab_id,
            AGENT,
            "session-live",
            Some("binding-switch"),
        )
        .expect("the resumed provider reclaims its existing session");
        assert!(claim.changed);
        assert!(claim.nonce_verified);
        assert!(matches!(
            fixture.service.write_hook_tab_agent_type_from_claim(
                &tab_id,
                AGENT.into(),
                Some(&tab_id),
                claim.identity_evidence(),
            ),
            Some(
                crate::service::provider_identity::AgentTypeWriteOutcome::Applied { .. }
                    | crate::service::provider_identity::AgentTypeWriteOutcome::Kept { .. }
            )
        ));
        assert_eq!(
            fixture
                .service
                .mux
                .read()
                .get_tab(&tab_id)
                .and_then(|tab| tab.pending_model_switch_observation.as_ref()),
            Some(&observation)
        );

        fixture
            .service
            .record_launch_reported_model(&tab_id, AGENT, "session-live", "haiku");
        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);
        assert_eq!(session_model(&fixture.service.mux, &tab_id), None);
        assert!(fixture.service.notif.lock().list(false).is_empty());

        post_boundary_models
            .lock()
            .unwrap()
            .insert("session-live".into(), "claude-sonnet-5-5".into());
        fixture
            .service
            .record_post_launch_turn_model(&tab_id, observation, "binding-switch");

        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_session_model.as_deref(), Some("sonnet"));
        assert!(tab
            .agent_session_model_identity
            .as_ref()
            .is_some_and(|identity| identity.model_switch_turn_confirmed));
        assert!(tab.pending_model_switch_observation.is_none());
        assert!(tab.model_fallback.is_none());
        assert!(tab.model_identity_mismatch.is_none());
        drop(mux);
        assert!(fixture.service.notif.lock().list(false).is_empty());
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn claude_worker_tab_detects_its_own_session_model() {
        let (mut mux, tab_id) = seeded_tab(Some("claude-worker-own"));
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("claude".into());
        tab.role = Some(crate::multiplexer::types::AgentRole::Worker);
        tab.session_binding_nonce = Some("worker-nonce".into());
        tab.reported_session_binding_nonce = Some("worker-nonce".into());
        let (provider, models) = SessionModelProvider::with_provider(
            "claude",
            &["claude-worker-own", "claude-foreign-newest"],
        );
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(provider));

        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);
        assert_eq!(session_model(&fixture.service.mux, &tab_id), None);

        models
            .lock()
            .unwrap()
            .insert("claude-worker-own".into(), "claude-opus-5".into());
        models
            .lock()
            .unwrap()
            .insert("claude-foreign-newest".into(), "claude-sonnet-5".into());
        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);

        assert_eq!(
            session_model(&fixture.service.mux, &tab_id).as_deref(),
            Some("opus")
        );
        assert_eq!(
            fixture
                .service
                .mux
                .read()
                .get_tab(&tab_id)
                .unwrap()
                .agent_session_model_identity
                .as_ref()
                .map(|identity| identity.session_id.as_str()),
            Some("claude-worker-own")
        );
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn stop_skips_session_model_detection_when_tab_model_is_known() {
        use crate::plan::{OrchestratorRef, Plan};
        use crate::service::plan::NewTask;

        let (mut mux, tab_id) = seeded_tab(Some("session-known"));
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.session_binding_nonce = Some("nonce-known".into());
        tab.reported_session_binding_nonce = Some("nonce-known".into());
        tab.agent_session_model = Some("opus".into());
        tab.agent_session_model_identity = Some(crate::multiplexer::types::SessionModelIdentity {
            session_id: "session-known".into(),
            session_binding_nonce: "nonce-known".into(),
            launch_report: None,
            model_switch_turn_confirmed: false,
        });
        let (provider, detection_calls) =
            SessionModelProvider::counting(&[("session-known", "claude-opus-5")]);
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(provider));
        let mut plan = Plan::new(
            "Known session model".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab("orchestrator"),
        );
        fixture.service.plan_store.save(&mut plan, 1).unwrap();
        let task_id = fixture
            .service
            .plan_store
            .add_task(
                &plan.id,
                NewTask {
                    name: "replay cached session model".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        fixture
            .service
            .plan_store
            .assign_task(&plan.id, &task_id, &tab_id, None, 3)
            .unwrap();
        let attempt_id = fixture
            .service
            .plan_store
            .show_task(&task_id)
            .unwrap()
            .task
            .execution_attempts[0]
            .id
            .clone();
        fixture
            .service
            .plan_store
            .update_attempt_executor_identity(
                &plan.id,
                &task_id,
                &tab_id,
                None,
                Some("session-known"),
                Some("nonce-known"),
                4,
            )
            .unwrap();

        assert!(!fixture
            .service
            .schedule_session_model_refresh_after_stop(&tab_id));
        assert_eq!(detection_calls.load(Ordering::Relaxed), 0);
        let attempt = fixture
            .service
            .plan_store
            .show_task(&task_id)
            .unwrap()
            .task
            .execution_attempts
            .into_iter()
            .find(|attempt| attempt.id == attempt_id)
            .unwrap();
        assert_eq!(attempt.session_model.as_deref(), Some("opus"));
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn attempt_records_session_model_from_its_own_bound_session() {
        use crate::plan::{OrchestratorRef, Plan};
        use crate::service::plan::NewTask;

        let (mut mux, tab_id) = seeded_tab(Some("session-owned"));
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.session_binding_nonce = Some("nonce-owned".into());
        tab.reported_session_binding_nonce = Some("nonce-owned".into());
        tab.agent_requested_model = Some("sonnet".into());
        tab.agent_provider_resolved_model = Some("sonnet".into());
        let fixture = ServiceFixture::new(mux);
        let workspace_id = fixture
            .service
            .mux
            .read()
            .find_workspace_for_tab(&tab_id)
            .unwrap();
        let unknown_tab = fixture
            .service
            .mux
            .write()
            .create_tab(&workspace_id)
            .unwrap();
        let foreign_tab = fixture
            .service
            .mux
            .write()
            .create_tab(&workspace_id)
            .unwrap();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&unknown_tab).unwrap();
            tab.cwd = Some(CWD.into());
            tab.agent_type = Some(AGENT.into());
            tab.agent_type_locked = true;
            tab.agent_session_id = Some("session-unknown".into());
            tab.session_binding_nonce = Some("nonce-unknown".into());
            tab.reported_session_binding_nonce = Some("nonce-unknown".into());
            let tab = mux.get_tab_mut(&foreign_tab).unwrap();
            tab.cwd = Some(CWD.into());
            tab.agent_type = Some(AGENT.into());
            tab.agent_type_locked = true;
            tab.agent_session_id = Some("foreign-newer".into());
            tab.session_binding_nonce = Some("nonce-foreign-tab".into());
            tab.reported_session_binding_nonce = Some("nonce-foreign-tab".into());
        }
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(SessionModelProvider::new(&[
                ("session-owned", "claude-opus-5"),
                ("foreign-newer", "claude-sonnet-5"),
            ])));
        let mut plan = Plan::new(
            "attempt session models".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab("orchestrator"),
        );
        fixture.service.plan_store.save(&mut plan, 1).unwrap();
        let add_and_bind = |tab: &str, session: &str, nonce: &str, now_ms| {
            let task_id = fixture
                .service
                .plan_store
                .add_task(
                    &plan.id,
                    NewTask {
                        name: format!("task for {tab}"),
                        ..Default::default()
                    },
                    now_ms,
                )
                .unwrap();
            fixture
                .service
                .plan_store
                .assign_task(&plan.id, &task_id, tab, None, now_ms + 1)
                .unwrap();
            fixture
                .service
                .plan_store
                .update_attempt_executor_identity(
                    &plan.id,
                    &task_id,
                    tab,
                    None,
                    Some(session),
                    Some(nonce),
                    now_ms + 2,
                )
                .unwrap();
            task_id
        };
        let foreign_session_task =
            add_and_bind(&tab_id, "foreign-newer", "nonce-foreign-session", 2);
        let nonce_mismatch_task = add_and_bind(&tab_id, "session-owned", "nonce-foreign", 5);
        let own_task = add_and_bind(&tab_id, "session-owned", "nonce-owned", 8);
        let unknown_task = add_and_bind(&unknown_tab, "session-unknown", "nonce-unknown", 11);
        let foreign_tab_task = add_and_bind(&foreign_tab, "foreign-newer", "nonce-foreign-tab", 14);

        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .agent_session_model = Some("stale-restored-model".into());
        fixture
            .service
            .persist_current_attempt_session_model(&tab_id);
        let stale_attempt = fixture
            .service
            .plan_store
            .list_task_attempts(&plan.id, &own_task)
            .unwrap()
            .remove(0);
        assert_eq!(stale_attempt.attempt.session_model, None);

        fixture
            .service
            .refresh_session_model_for_current_tab(&foreign_tab);
        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);
        fixture
            .service
            .refresh_session_model_for_current_tab(&unknown_tab);

        let own_attempt = fixture
            .service
            .plan_store
            .list_task_attempts(&plan.id, &own_task)
            .unwrap()
            .remove(0);
        let unknown_attempt = fixture
            .service
            .plan_store
            .list_task_attempts(&plan.id, &unknown_task)
            .unwrap()
            .remove(0);
        let foreign_session_attempt = fixture
            .service
            .plan_store
            .list_task_attempts(&plan.id, &foreign_session_task)
            .unwrap()
            .remove(0);
        let nonce_mismatch_attempt = fixture
            .service
            .plan_store
            .list_task_attempts(&plan.id, &nonce_mismatch_task)
            .unwrap()
            .remove(0);
        let foreign_tab_attempt = fixture
            .service
            .plan_store
            .list_task_attempts(&plan.id, &foreign_tab_task)
            .unwrap()
            .remove(0);
        assert_eq!(
            own_attempt.attempt.session_model.as_deref(),
            Some("opus"),
            "a newer foreign session must not replace the tab's own model"
        );
        assert_eq!(
            own_attempt.attempt.requested_model.as_deref(),
            Some("sonnet")
        );
        assert_eq!(
            own_attempt.attempt.provider_resolved_model.as_deref(),
            Some("opus")
        );
        let mismatch = own_attempt
            .attempt
            .model_identity_mismatch
            .as_ref()
            .unwrap();
        assert_eq!(mismatch.requested_model, "sonnet");
        assert_eq!(mismatch.provider_resolved_model.as_deref(), Some("sonnet"));
        assert_eq!(mismatch.actual_model, "opus");
        let serialized = serde_json::to_value(&own_attempt.attempt).unwrap();
        assert_eq!(serialized["requested_model"], "sonnet");
        assert_eq!(serialized["provider_resolved_model"], "opus");
        assert_eq!(serialized["session_model"], "opus");
        assert_eq!(
            serialized["model_identity_mismatch"]["actual_model"],
            "opus"
        );
        assert_eq!(foreign_session_attempt.attempt.session_model, None);
        assert_eq!(nonce_mismatch_attempt.attempt.session_model, None);
        assert_eq!(
            foreign_tab_attempt.attempt.session_model.as_deref(),
            Some("sonnet")
        );
        assert_eq!(unknown_attempt.attempt.session_model, None);
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn session_model_detected_before_binding_is_replayed_when_attempt_becomes_eligible() {
        use crate::plan::{OrchestratorRef, Plan};
        use crate::service::plan::NewTask;

        let (mut mux, tab_id) = seeded_tab(Some("session-replay"));
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.session_binding_nonce = Some("nonce-replay".into());
        tab.reported_session_binding_nonce = Some("nonce-replay".into());
        tab.agent_session_model = Some("gpt-6-luna".into());
        tab.agent_session_model_identity = Some(crate::multiplexer::types::SessionModelIdentity {
            session_id: "session-replay".into(),
            session_binding_nonce: "nonce-replay".into(),
            launch_report: None,
            model_switch_turn_confirmed: false,
        });
        let fixture = ServiceFixture::new(mux);
        let mut plan = Plan::new(
            "model replay".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab("orchestrator"),
        );
        fixture.service.plan_store.save(&mut plan, 1).unwrap();
        let task_id = fixture
            .service
            .plan_store
            .add_task(
                &plan.id,
                NewTask {
                    name: "replay model".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        fixture
            .service
            .plan_store
            .assign_task(&plan.id, &task_id, &tab_id, None, 3)
            .unwrap();
        let attempt_id = fixture
            .service
            .plan_store
            .show_task(&task_id)
            .unwrap()
            .task
            .execution_attempts[0]
            .id
            .clone();

        fixture
            .service
            .persist_current_attempt_session_model(&tab_id);
        assert_eq!(
            fixture
                .service
                .plan_store
                .show_task(&task_id)
                .unwrap()
                .task
                .execution_attempts[0]
                .session_model,
            None
        );
        crate::service::worker_identity::persist_attempt_executor_identity(
            &fixture.service.plan_store,
            &Arc::new(fixture.service.clone()),
            &plan.id,
            &task_id,
            &tab_id,
            "Worker",
            4,
        )
        .unwrap();

        let attempt = fixture
            .service
            .plan_store
            .show_task(&task_id)
            .unwrap()
            .task
            .execution_attempts
            .into_iter()
            .find(|attempt| attempt.id == attempt_id)
            .unwrap();
        assert_eq!(attempt.session_model.as_deref(), Some("gpt-6-luna"));
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn unbound_attempt_model_refresh_does_not_rewrite_its_plan() {
        use crate::plan::{OrchestratorRef, Plan};
        use crate::service::plan::NewTask;

        let (mut mux, tab_id) = seeded_tab(Some("session-live"));
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.session_binding_nonce = Some("nonce-live".into());
        tab.reported_session_binding_nonce = Some("nonce-live".into());
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(SessionModelProvider::new(&[(
                "session-live",
                "claude-opus-5",
            )])));
        let mut plan = Plan::new(
            "unbound attempt refresh".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab("orchestrator"),
        );
        fixture.service.plan_store.save(&mut plan, 1).unwrap();
        let task_id = fixture
            .service
            .plan_store
            .add_task(
                &plan.id,
                NewTask {
                    name: "unbound worker".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        fixture
            .service
            .plan_store
            .assign_task(&plan.id, &task_id, &tab_id, None, 3)
            .unwrap();
        let before = fixture.service.plan_store.get(&plan.id).unwrap();

        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);

        let after = fixture.service.plan_store.get(&plan.id).unwrap();
        assert_eq!(after.updated_at, before.updated_at);
        assert_eq!(after.tasks[0].execution_attempts[0].session_model, None,);
    }

    /// The rejection half: the session rolls between the detector read and the
    /// guarded write, so the answer now describes a retired session.
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn model_badge_rejects_answer_from_superseded_session() {
        let (mux, tab_id) = seeded_tab(Some("session-live"));
        let fixture = ServiceFixture::new(mux);
        let racing_mux: MuxState = Arc::clone(&fixture.service.mux);
        let racing_tab_id = tab_id.clone();
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(SessionModelProvider::racing(
                &[("session-live", "claude-opus-5")],
                move || {
                    if let Some(tab) = racing_mux.write().get_tab_mut(&racing_tab_id) {
                        tab.agent_session_id = Some("session-next".into());
                    }
                },
            )));

        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);

        assert_eq!(
            session_model(&fixture.service.mux, &tab_id),
            None,
            "a result computed for the retired session must not overwrite the tab"
        );
        assert_eq!(
            fixture
                .service
                .mux
                .read()
                .get_tab(&tab_id)
                .and_then(|tab| tab.agent_session_id.clone())
                .as_deref(),
            Some("session-next")
        );
    }

    /// Same race on the provider axis: a prior PTY's agent answered after the
    /// tab was already relaunched onto a different provider.
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn model_badge_rejects_answer_for_foreign_agent() {
        let (mux, tab_id) = seeded_tab(Some("session-live"));
        let fixture = ServiceFixture::new(mux);
        let racing_mux: MuxState = Arc::clone(&fixture.service.mux);
        let racing_tab_id = tab_id.clone();
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(SessionModelProvider::racing(
                &[("session-live", "claude-opus-5")],
                move || {
                    if let Some(tab) = racing_mux.write().get_tab_mut(&racing_tab_id) {
                        tab.agent_type = Some("other-agent".into());
                    }
                },
            )));

        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);

        assert_eq!(
            session_model(&fixture.service.mux, &tab_id),
            None,
            "an answer for a provider the tab no longer runs must be dropped"
        );
    }

    /// The reported disappearance: startup spends its single attempt while the
    /// tab is still unbound, and the binding commit clears runtime evidence.
    /// The binding event is what puts the model back.
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn model_badge_after_session_binding_recovers_unbound_startup_tab() {
        let (mux, tab_id) = seeded_tab(None);
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(SessionModelProvider::new(&[(
                "session-bound",
                "claude-opus-5",
            )])));

        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);
        assert_eq!(
            session_model(&fixture.service.mux, &tab_id),
            None,
            "an unbound tab has no session to attribute a model to"
        );

        crate::api::hook_routes::bind_reported_session(
            &fixture.service.mux,
            fixture.service.agent_registry.as_ref(),
            &tab_id,
            AGENT,
            "session-bound",
            None,
        )
        .unwrap();
        assert_eq!(
            session_model(&fixture.service.mux, &tab_id),
            None,
            "the binding commit itself carries no model evidence"
        );

        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);

        assert_eq!(
            session_model(&fixture.service.mux, &tab_id).as_deref(),
            Some("opus"),
            "the bound session's model must survive the startup/binding order"
        );
    }

    /// Fallback and mismatch survive the repair: a runtime model that differs
    /// from the launch request still records both, with the model committed.
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn session_model_mismatch_is_recorded_and_surfaced() {
        let (mut mux, tab_id) = seeded_tab(Some("session-live"));
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_requested_model = Some("sonnet".into());
        tab.agent_provider_resolved_model = Some("sonnet".into());
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(SessionModelProvider::new(&[(
                "session-live",
                "claude-opus-5",
            )])));

        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);

        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_session_model.as_deref(), Some("opus"));
        assert_eq!(tab.agent_provider_resolved_model.as_deref(), Some("opus"));
        let mismatch = tab
            .model_identity_mismatch
            .as_ref()
            .expect("a runtime model unlike the request stays a structured mismatch");
        assert_eq!(mismatch.requested_model, "sonnet");
        assert_eq!(mismatch.actual_model, "opus");
        assert_eq!(mismatch.reason, "runtime_session_model_mismatch");
        assert_eq!(
            tab.model_fallback
                .as_ref()
                .and_then(|fallback| fallback.actual_model.as_deref()),
            Some("opus")
        );
        drop(mux);
        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);
        let notif = fixture.service.notif.lock();
        let notifications = notif.list(false);
        assert_eq!(notifications.len(), 1);
        assert_eq!(
            notifications[0].title,
            "model fallback: requested sonnet, actual opus"
        );
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn qualified_catalogue_model_matches_bare_runtime_model_without_notice() {
        let (mut mux, tab_id) = seeded_tab(Some("session-live"));
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("opencode".into());
        tab.agent_requested_model = Some("opencode/big-pickle".into());
        tab.agent_provider_resolved_model = Some("opencode/big-pickle".into());
        let fixture = ServiceFixture::new(mux);
        let (provider, sessions) =
            SessionModelProvider::with_provider("opencode", &["session-live"]);
        sessions
            .lock()
            .unwrap()
            .insert("session-live".into(), "big-pickle".into());
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(provider));
        crate::agent::model_catalog::put_live(
            "opencode",
            vec![crate::agent::provider::ModelInfo {
                id: "opencode/big-pickle".into(),
                display_name: "Big Pickle".into(),
                ..Default::default()
            }],
            Some("opencode/big-pickle".into()),
        );

        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);

        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_session_model.as_deref(), Some("big-pickle"));
        assert_eq!(
            tab.agent_provider_resolved_model.as_deref(),
            Some("big-pickle")
        );
        assert!(tab.model_identity_mismatch.is_none());
        drop(mux);
        assert!(fixture.service.notif.lock().list(false).is_empty());
        crate::agent::model_catalog::unregister("opencode");
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    #[test]
    fn provider_resolved_model_reports_actual_not_requested() {
        let (mut mux, tab_id) = seeded_tab(Some("session-live"));
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_requested_model = Some("sonnet".into());
        tab.agent_provider_resolved_model = Some("sonnet".into());
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(SessionModelProvider::new(&[(
                "session-live",
                "claude-opus-5",
            )])));

        fixture
            .service
            .refresh_session_model_for_current_tab(&tab_id);

        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_requested_model.as_deref(), Some("sonnet"));
        assert_eq!(tab.agent_provider_resolved_model.as_deref(), Some("opus"));
        assert_eq!(tab.agent_session_model.as_deref(), Some("opus"));
    }
}
