use super::*;
use crate::service::agent_lifecycle::{ModelSwitchConfirmation, RestartTracker};
use crate::service::agent_restart::model_switch::ModelSwitchExt;
use std::time::{Duration, Instant};

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
fn model_restart_preserves_owned_browser_until_explicit_stop() {
    use codeterm_plugin::surfaces::SurfaceRegistryHandle;

    for process_exits_before_restart in [false, true] {
        let fixture = RuntimeFixture::new();
        let svc = &fixture.service;
        let tab = &fixture.tab;
        svc.ext::<SurfaceRegistryHandle>().register(
            "fixture",
            Arc::new(
                serde_json::from_value(json!([{
                    "id": "page", "title": "Browser", "renderer": "stream", "runtime": "browser",
                    "agentBound": true
                }]))
                .unwrap(),
            ),
        );
        let browser = svc
            .open_surface(crate::service::surface::SurfaceOpenRequest {
                target: "fixture:page".into(),
                owner_tab_id: Some(tab.clone()),
                ..Default::default()
            })
            .unwrap();
        let prior = agent_runtime::engine::for_tab(tab).unwrap();
        let process = prior.process.lock().clone().unwrap();
        let wait_for_retirement = || {
            crate::async_runtime::block_on(async {
                tokio::time::timeout(
                    Duration::from_secs(5),
                    process.client.wait_for_subscriber_exit_for_test(),
                )
                .await
                .expect("old process subscriber finished");
            })
        };
        if process_exits_before_restart {
            crate::async_runtime::block_on(process.finish()).unwrap();
            wait_for_retirement();
        }

        let outcome = svc.agent_switch_model(tab, "selected-model").unwrap();
        assert!(outcome.restart.unwrap().agent_ready);
        wait_for_retirement();
        assert_eq!(svc.list_surfaces().open, [browser]);

        let resumed = agent_runtime::engine::for_tab(tab).unwrap();
        let replacement = resumed.process.lock().clone().unwrap();
        assert!(!Arc::ptr_eq(&process, &replacement));
        assert!(replacement.ended.borrow().is_none());
        svc.kill_tab_pty(tab).unwrap();
        assert!(replacement.ended.borrow().is_some());
        assert!(svc.list_surfaces().open.is_empty());
    }
}

#[test]
fn consecutive_restart_resume_switches_apply_without_cooldown() {
    let fixture = RuntimeFixture::new();
    let svc = &fixture.service;
    let tab = &fixture.tab;
    for model in ["first-model", "last-model"] {
        let outcome = svc.agent_switch_model(tab, model).unwrap();
        assert!(outcome.restart.as_ref().unwrap().agent_ready);
        assert!(
            matches!(outcome.confirmation, ModelSwitchConfirmation::Observed { model: observed, .. } if observed == model)
        );
    }
    assert_eq!(resumed_models(&fixture), ["first-model", "last-model"]);
    assert_eq!(
        svc.mux
            .read()
            .get_tab(tab)
            .unwrap()
            .restorable_launch
            .model
            .as_deref(),
        Some("last-model")
    );
}

#[test]
fn queued_restart_resume_switches_coalesce_until_the_restart_settles() {
    let fixture = RuntimeFixture::new();
    let svc = &fixture.service;
    let tab = &fixture.tab;
    let (entered, release) = agent_runtime::engine::hold_publication_for_test(tab);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| svc.agent_switch_model(tab, "first-model"));
        if let Err(error) = entered.recv_timeout(Duration::from_secs(5)) {
            let _ = release.send(());
            panic!("first switch did not reach runtime publication: {error}");
        }
        let queued =
            ["superseded-model", "last-model"].map(|model| svc.agent_switch_model(tab, model));
        let pending = svc
            .mux
            .read()
            .get_tab(tab)
            .unwrap()
            .pending_model_selection
            .clone();
        release.send(()).unwrap();
        let first = first.join().unwrap().unwrap();
        assert!(first.restart.as_ref().unwrap().agent_ready);
        for outcome in queued {
            assert_eq!(
                outcome.unwrap().confirmation,
                ModelSwitchConfirmation::AfterTurn
            );
        }
        assert_eq!(pending.unwrap().model, "last-model");
    });
    assert!(crate::service::boot_wait::wait_until(
        Instant::now() + Duration::from_secs(5),
        || {
            let busy = svc.ext::<RestartTracker>().lock().model_switch_busy(tab);
            !busy
                && svc.mux.read().get_tab(tab).is_some_and(|tab| {
                    tab.pending_model_selection.is_none()
                        && tab.restorable_launch.model.as_deref() == Some("last-model")
                })
        }
    ));
    assert_eq!(resumed_models(&fixture), ["first-model", "last-model"]);
    assert_eq!(
        svc.mux
            .read()
            .get_tab(tab)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("own-thread")
    );
}

fn resumed_models(fixture: &RuntimeFixture) -> Vec<String> {
    fixture
        .requests()
        .into_iter()
        .filter(|frame| frame["method"] == "resumeThread")
        .filter_map(|frame| {
            frame["params"]["settings"]["model"]
                .as_str()
                .map(str::to_owned)
        })
        .filter(|model| model != "supplied-model")
        .collect()
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
