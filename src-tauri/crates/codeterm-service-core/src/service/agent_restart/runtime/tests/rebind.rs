use super::*;
use agent_runtime::model::RuntimeFailure;
use codeterm_types::pty::BootStage;

fn rebind(fixture: &RuntimeFixture, thread: &str) {
    let mut mux = fixture.service.mux.write();
    let evidence = crate::agent::provider_identity::ProviderIdentityEvidence::capture(
        mux.get_tab(&fixture.tab).unwrap(),
    );
    let claim = crate::service::hook_session_binding::commit_materialized_rebind_session(
        &mut mux,
        &fixture.tab,
        PROVIDER,
        thread,
        None,
        &evidence,
    )
    .unwrap();
    assert!(claim.changed);
    assert_eq!(
        mux.get_tab(&fixture.tab)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some(thread)
    );
}

#[test]
fn rebind_then_restart_resumes_recorded_thread_and_its_history_in_both_directions() {
    let fixture = RuntimeFixture::new();
    let original = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    let original_process = original.process.lock().clone().unwrap();
    let original_context = original.context.lock().clone();
    let count = fixture.requests().len();
    rebind(&fixture, "earlier-thread");
    assert!(matches!(
        agent_runtime::engine::validate_binding(&fixture.service, &original),
        Err(RuntimeFailure::Superseded)
    ));

    for thread in ["earlier-thread", "own-thread"] {
        if thread == "own-thread" {
            rebind(&fixture, thread);
            fixture
                .service
                .ext::<crate::service::agent_restart::RestartTracker>()
                .lock()
                .last_restart
                .remove(&fixture.tab);
        }
        let before = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
        let before_context = before.context.lock().clone();
        let before_process = before.process.lock().clone().unwrap();
        let outcome = fixture
            .service
            .agent_restart(&fixture.tab, false, 0)
            .unwrap();
        assert!(outcome.agent_ready && outcome.binding_restored);
        assert_eq!(outcome.session_action(), "resumed");
        assert_eq!(outcome.session_id.as_deref(), Some(thread));
        assert!(outcome.old_pty_id.is_none() && outcome.new_pty_id.is_none());
        assert!(before_process.ended.borrow().is_some());
        assert!(matches!(
            agent_runtime::engine::validate_binding(&fixture.service, &before),
            Err(RuntimeFailure::Superseded)
        ));
        let current = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
        assert!(!Arc::ptr_eq(&current, &before));
        let context = current.context.lock().clone();
        assert_eq!(context.thread_id.as_deref(), Some(thread));
        assert_ne!(context.binding_nonce, before_context.binding_nonce);
        assert!(context.epoch > before_context.epoch);
        let history = current.cache.lock().snapshot();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, format!("{thread} history"));
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&fixture.tab).unwrap();
        assert_eq!(tab.agent_session_id.as_deref(), Some(thread));
        assert_eq!(
            tab.reported_session_binding_nonce,
            tab.session_binding_nonce
        );
        assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
        assert_eq!(tab.agent_runtime_state.as_ref().unwrap()["state"], "loaded");
        assert!(
            matches!(&tab.backing, TabBacking::ChatBackend {session_id,..} if session_id == thread)
        );
    }
    assert!(original_process.ended.borrow().is_some());
    assert_eq!(
        original.context.lock().thread_id,
        original_context.thread_id
    );
    let requests = fixture.requests();
    assert_eq!(
        requests[count..]
            .iter()
            .map(|request| request["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "initialize",
            "resumeThread",
            "readHistory",
            "initialize",
            "resumeThread",
            "readHistory"
        ]
    );
    assert_eq!(requests[count + 1]["params"]["threadId"], "earlier-thread");
    assert_eq!(requests[count + 4]["params"]["threadId"], "own-thread");
}

#[test]
fn degraded_rebound_runtime_has_an_honest_resume_refusal_and_restart_recovery() {
    let fixture = RuntimeFixture::new();
    rebind(&fixture, "earlier-thread");
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        tab.boot_stage = Some(BootStage::SpawnFailed);
        tab.agent_runtime_state =
            Some(json!({"state":"degraded","epoch":5,"reason":"opaque failure"}));
    }
    let before = fixture.requests().len();
    let error = fixture
        .service
        .existing_session_resume_tab("earlier-thread")
        .unwrap_err();
    assert!(
        error.contains(&format!("codeterm agent restart --tab {}", fixture.tab)),
        "{error}"
    );
    assert_eq!(fixture.requests().len(), before);
    let outcome = fixture
        .service
        .agent_restart(&fixture.tab, false, 0)
        .unwrap();
    assert_eq!(outcome.session_id.as_deref(), Some("earlier-thread"));
    assert!(outcome.agent_ready);
    assert_eq!(
        fixture
            .service
            .existing_session_resume_tab("earlier-thread")
            .unwrap(),
        Some(fixture.tab.clone())
    );
}

#[test]
fn runtime_rebind_restart_still_refuses_foreign_launch_nonce() {
    let fixture = RuntimeFixture::new();
    rebind(&fixture, "earlier-thread");
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.tab)
        .unwrap()
        .session_binding_nonce = Some("foreign-nonce".into());
    let count = fixture.requests().len();
    assert!(fixture
        .service
        .agent_restart(&fixture.tab, false, 0)
        .is_err());
    assert_eq!(fixture.requests().len(), count);
    assert_eq!(
        agent_runtime::engine::for_tab(&fixture.tab)
            .unwrap()
            .context
            .lock()
            .thread_id
            .as_deref(),
        Some("own-thread")
    );
}

#[test]
fn runtime_rebind_needs_explicit_restart_and_keeps_normal_thread_guards() {
    let fixture = RuntimeFixture::new();
    rebind(&fixture, "earlier-thread");
    let count = fixture.requests().len();
    assert!(
        crate::async_runtime::block_on(agent_runtime::engine::ensure(
            &fixture.service,
            &fixture.tab
        ))
        .is_err()
    );
    assert!(
        crate::async_runtime::block_on(agent_runtime::restart::restart_idle(
            &fixture.service,
            &fixture.tab,
            PROVIDER,
            "own-nonce",
        ))
        .is_err()
    );
    assert_eq!(fixture.requests().len(), count);
    let outcome = fixture
        .service
        .agent_restart(&fixture.tab, false, 0)
        .unwrap();
    assert!(outcome.agent_ready);
    assert_eq!(outcome.session_id.as_deref(), Some("earlier-thread"));
}

#[test]
fn refused_rebound_thread_preserves_target_and_can_retry_without_fresh_fallback() {
    let fixture = RuntimeFixture::new();
    rebind(&fixture, "earlier-thread");
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.tab)
        .unwrap()
        .agent_runtime_config = Some(json!({"systemPrompt":"opaque refused identity"}));
    assert!(fixture
        .service
        .agent_restart(&fixture.tab, false, 0)
        .is_err());
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&fixture.tab).unwrap();
        assert_eq!(tab.agent_session_id.as_deref(), Some("earlier-thread"));
        assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
        assert_eq!(
            tab.agent_runtime_state.as_ref().unwrap()["state"],
            "degraded"
        );
    }
    assert!(fixture
        .service
        .existing_session_resume_tab("earlier-thread")
        .is_err());
    fixture
        .service
        .ext::<crate::service::agent_restart::RestartTracker>()
        .lock()
        .last_restart
        .remove(&fixture.tab);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.tab)
        .unwrap()
        .agent_runtime_config = Some(json!({"systemPrompt":PROMPT}));
    let outcome = fixture
        .service
        .agent_restart(&fixture.tab, false, 0)
        .unwrap();
    assert!(outcome.agent_ready);
    assert_eq!(outcome.session_id.as_deref(), Some("earlier-thread"));
    assert!(fixture
        .requests()
        .iter()
        .all(|request| request["method"] != "openThread"));
}
