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
        models: HashMap<String, String>,
        sessions: HashSet<String>,
        during_detect: Option<Box<dyn Fn() + Send + Sync>>,
    }

    impl SessionModelProvider {
        fn new(models: &[(&str, &str)]) -> Self {
            Self {
                models: models
                    .iter()
                    .map(|(sid, model)| ((*sid).to_string(), (*model).to_string()))
                    .collect(),
                sessions: models.iter().map(|(sid, _)| (*sid).to_string()).collect(),
                during_detect: None,
            }
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
            AGENT
        }
        fn display_name(&self) -> &str {
            AGENT
        }
        fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
            AGENT.to_string()
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
            Some(format!("{AGENT} --resume {session_id}"))
        }
        fn detect_session_model(&self, _: &str, session_id: &str) -> Option<String> {
            if let Some(during_detect) = self.during_detect.as_ref() {
                during_detect();
            }
            self.models.get(session_id).cloned()
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
        let (mux, tab_id) = seeded_tab(Some("session-live"));
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(SessionModelProvider::new(&[(
                "session-live",
                "claude-opus-5",
            )])));

        fixture.service.refresh_session_model_after_binding(&tab_id);

        assert_eq!(
            session_model(&fixture.service.mux, &tab_id).as_deref(),
            Some("opus"),
            "an answer for the tab's current session must reach the tab"
        );
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

        fixture.service.refresh_session_model_after_binding(&tab_id);

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

        fixture.service.refresh_session_model_after_binding(&tab_id);

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

        fixture.service.refresh_session_model_after_binding(&tab_id);
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

        fixture.service.refresh_session_model_after_binding(&tab_id);

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

        fixture.service.refresh_session_model_after_binding(&tab_id);

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
