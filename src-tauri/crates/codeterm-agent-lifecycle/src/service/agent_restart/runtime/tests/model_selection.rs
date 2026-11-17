use super::*;
use crate::service::agent_restart::model_switch::ModelSwitchExt;

#[test]
fn restart_resume_model_selection_reloads_the_exact_runtime_thread_without_live_control() {
    for service_entry in [false, true] {
        let fixture = RuntimeFixture::new();
        let prior = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
        let nonce = prior.context.lock().binding_nonce.clone();
        let outcome = if service_entry {
            fixture.service.agent_switch_model_with_restart_fallback(
                &fixture.tab,
                "selected-model",
                None,
                None,
                agent_runtime::model::ModelSwitchOptions {
                    force: false,
                    restart_resume: false,
                    service_tier: None,
                },
            )
        } else {
            fixture
                .service
                .agent_switch_model(&fixture.tab, "selected-model")
        }
        .unwrap();
        assert_eq!(
            outcome.mechanism,
            crate::agent::provider::ModelSwitchMode::RestartResume
        );
        assert_eq!(outcome.session_id, "own-thread");
        assert!(outcome.restart.as_ref().unwrap().agent_ready);
        assert!(
            matches!(outcome.confirmation, crate::service::agent_lifecycle::ModelSwitchConfirmation::Observed {model, ..} if model == "selected-model")
        );
        let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
        let ctx = session.context.lock().clone();
        assert_ne!(ctx.binding_nonce, nonce);
        assert_eq!(ctx.thread_id.as_deref(), Some("own-thread"));
        assert_eq!(ctx.settings.model.as_deref(), Some("selected-model"));
        let requests = fixture.requests();
        assert!(!requests.iter().any(|frame| frame["method"] == "control"
            || frame["method"] == "submitTurn"
            || frame["method"] == "openThread"));
        assert!(requests
            .iter()
            .any(|frame| frame["method"] == "resumeThread"
                && frame["params"]["settings"]["model"] == "selected-model"));
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&fixture.tab).unwrap();
        assert_eq!(
            tab.restorable_launch.model.as_deref(),
            Some("selected-model")
        );
        assert_eq!(
            tab.agent_runtime_config.as_ref().unwrap()["model"],
            "selected-model"
        );
    }
}

#[test]
fn busy_restart_resume_selection_preserves_model_process_and_binding() {
    let fixture = RuntimeFixture::new();
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    let nonce = session.context.lock().binding_nonce.clone();
    session.context.lock().active_turn_id = Some("working-turn".into());
    assert!(fixture
        .service
        .agent_switch_model(&fixture.tab, "selected-model")
        .is_err());
    assert!(!fixture
        .requests()
        .iter()
        .any(|frame| frame["method"] == "control"
            || frame["method"] == "resumeThread"
                && frame["params"]["settings"]["model"] == "selected-model"));
    assert!(session
        .thread_loaded
        .load(std::sync::atomic::Ordering::Acquire));
    assert!(matches!(
        session.state.lock().phase,
        agent_runtime::model::RuntimePhase::Ready
    ));
    let ctx = session.context.lock();
    assert_eq!(ctx.binding_nonce, nonce);
    assert_eq!(ctx.settings.model.as_deref(), Some("supplied-model"));
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .unwrap()
            .agent_runtime_config
            .as_ref()
            .unwrap()["model"],
        "supplied-model"
    );
}
