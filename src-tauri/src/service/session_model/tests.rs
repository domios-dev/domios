mod model_identity_tests {
    use crate::service::session_model::*;

    #[test]
    fn runtime_model_mismatch_records_actual_and_structured_reason() {
        let update = model_identity_update(
            None,
            Some("planned-codex"),
            Some("gpt-5.6-luna"),
            Some("gpt-5.6-luna"),
            None,
            "gpt-5.6-terra",
        );

        assert!(update.model_changed);
        let mismatch = update.mismatch.expect("mismatch must be structured");
        assert_eq!(mismatch.requested_model, "gpt-5.6-luna");
        assert_eq!(mismatch.planned_model.as_deref(), Some("planned-codex"));
        assert_eq!(
            mismatch.provider_resolved_model.as_deref(),
            Some("gpt-5.6-luna")
        );
        assert_eq!(mismatch.actual_model, "gpt-5.6-terra");
        assert_eq!(mismatch.reason, "runtime_session_model_mismatch");
        let fallback = update.fallback.expect("fallback must be surfaced");
        assert_eq!(fallback.actual_model.as_deref(), Some("gpt-5.6-terra"));
    }

    #[test]
    fn repeated_runtime_mismatch_keeps_actual_without_second_model_change() {
        let update = model_identity_update(
            Some("gpt-5.6-terra"),
            None,
            Some("gpt-5.6-luna"),
            Some("gpt-5.6-luna"),
            None,
            "gpt-5.6-terra",
        );

        assert!(!update.model_changed);
        assert_eq!(
            update.mismatch,
            Some(crate::multiplexer::types::ModelIdentityMismatch {
                planned_model: None,
                requested_model: "gpt-5.6-luna".into(),
                provider_resolved_model: Some("gpt-5.6-luna".into()),
                actual_model: "gpt-5.6-terra".into(),
                reason: "runtime_session_model_mismatch".into(),
            })
        );
    }

    #[test]
    fn matching_runtime_model_clears_mismatch_shape() {
        let update = model_identity_update(
            Some("gpt-5.6-luna"),
            None,
            Some("gpt-5.6-luna"),
            Some("gpt-5.6-luna"),
            None,
            "GPT-5.6-LUNA",
        );

        assert!(!update.model_changed);
        assert!(update.mismatch.is_none());
        assert!(update.fallback.is_none());
    }

    #[test]
    fn session_change_clears_stale_actual_but_keeps_launch_provenance() {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_planned_model = Some("codex/gpt-planned".into());
        tab.agent_requested_model = Some("gpt-requested".into());
        tab.agent_provider_resolved_model = Some("gpt-resolved".into());
        tab.agent_session_model = Some("gpt-actual-old".into());
        tab.model_identity_mismatch = Some(crate::multiplexer::types::ModelIdentityMismatch {
            planned_model: Some("codex/gpt-planned".into()),
            requested_model: "gpt-requested".into(),
            provider_resolved_model: Some("gpt-resolved".into()),
            actual_model: "gpt-actual-old".into(),
            reason: "runtime_session_model_mismatch".into(),
        });

        clear_runtime_model_evidence(tab);

        assert_eq!(
            tab.agent_planned_model.as_deref(),
            Some("codex/gpt-planned")
        );
        assert_eq!(tab.agent_requested_model.as_deref(), Some("gpt-requested"));
        assert_eq!(
            tab.agent_provider_resolved_model.as_deref(),
            Some("gpt-resolved")
        );
        assert!(tab.agent_session_model.is_none());
        assert!(tab.model_identity_mismatch.is_none());
    }

    #[test]
    fn delayed_runtime_model_result_cannot_cross_session_boundary() {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("provider-a".into());
        tab.agent_session_id = Some("session-new".into());

        assert!(!runtime_model_update_matches_session(
            tab,
            "provider-a",
            "session-old"
        ));
        assert!(!runtime_model_update_matches_session(
            tab,
            "provider-b",
            "session-new"
        ));
        assert!(runtime_model_update_matches_session(
            tab,
            "provider-a",
            "session-new"
        ));
    }
}

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
            release_rx.recv().unwrap();
        })
        .unwrap();

        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
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

    const AGENT: &str = "fixture-agent";
    const CWD: &str = "/fixture/project";

    /// Detector oracle: answers only for the session it is asked about, and can
    /// mutate the tab mid-detection to reproduce the real read/write race.
    struct SessionModelProvider {
        provider_id: String,
        models: Arc<std::sync::Mutex<HashMap<String, String>>>,
        sessions: HashSet<String>,
        during_detect: Option<Box<dyn Fn() + Send + Sync>>,
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
                sessions: models.iter().map(|(sid, _)| (*sid).to_string()).collect(),
                during_detect: None,
            }
        }

        fn with_provider(
            provider_id: &str,
            session_ids: &[&str],
        ) -> (Self, Arc<std::sync::Mutex<HashMap<String, String>>>) {
            let models = Arc::new(std::sync::Mutex::new(HashMap::new()));
            let provider = Self {
                provider_id: provider_id.into(),
                models: models.clone(),
                sessions: session_ids.iter().map(|sid| (*sid).into()).collect(),
                during_detect: None,
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
            if let Some(during_detect) = self.during_detect.as_ref() {
                during_detect();
            }
            self.models.lock().unwrap().get(session_id).cloned()
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

        fixture.service.refresh_session_model_for_current_tab(&tab_id);

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
            })
        );
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

        models.lock().unwrap().insert(
            "claude-worker-own".into(),
            "claude-opus-5".into(),
        );
        models.lock().unwrap().insert(
            "claude-foreign-newest".into(),
            "claude-sonnet-5".into(),
        );
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
    fn attempt_records_session_model_from_its_own_bound_session() {
        use crate::plan::{OrchestratorRef, Plan};
        use crate::service::plan::NewTask;

        let (mut mux, tab_id) = seeded_tab(Some("session-owned"));
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.session_binding_nonce = Some("nonce-owned".into());
        tab.reported_session_binding_nonce = Some("nonce-owned".into());
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
        fixture.service.refresh_session_model_for_current_tab(&tab_id);
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
        tab.agent_session_model_identity = Some(
            crate::multiplexer::types::SessionModelIdentity {
                session_id: "session-replay".into(),
                session_binding_nonce: "nonce-replay".into(),
            },
        );
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

        fixture.service.refresh_session_model_for_current_tab(&tab_id);

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

        fixture.service.refresh_session_model_for_current_tab(&tab_id);

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

        fixture.service.refresh_session_model_for_current_tab(&tab_id);

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

        fixture.service.refresh_session_model_for_current_tab(&tab_id);
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

        fixture.service.refresh_session_model_for_current_tab(&tab_id);

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
    fn model_badge_keeps_fallback_and_mismatch_for_current_session() {
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

        fixture.service.refresh_session_model_for_current_tab(&tab_id);

        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_session_model.as_deref(), Some("opus"));
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
    }
}
