use super::*;
use crate::service::test_fixture::ServiceFixture;
use serde_json::json;

fn fixture(auto_approve: bool, permission_auto_confirm: bool) -> (ServiceFixture, String) {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.auto_approve = auto_approve;
    tab.permission_auto_confirm = permission_auto_confirm;
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    tab.restored_from_snapshot = false;
    super::super::register_endpoint(&tab_id, "http://127.0.0.1:1".into(), "/work".into());
    super::super::bind_launch(&tab_id, "own-nonce");
    (ServiceFixture::new(mux), tab_id)
}

fn permission_event() -> serde_json::Value {
    json!({"type":"permission.asked","properties":{"id":"own-request","sessionID":"own-session","permission":"opaque"}})
}

fn insert(tab_id: &str, event: serde_json::Value) {
    let super::super::ParsedEvent::Prompt { mut prompt, route } = super::super::parse_event(&event) else { panic!("prompt"); };
    prompt.request_id = Some(route.request_id.clone());
    ACTIVE.lock().insert(tab_id.into(), Active { prompt, route, in_flight: false, automatic_nonce: None });
}

#[test]
fn automatic_permission_truth_table_preserves_questions_and_manual_controls() {
    for (auto_approve, permission_auto_confirm, automatic) in [
        (false, false, false), (true, false, true), (false, true, true), (true, true, true),
    ] {
        let (fixture, tab_id) = fixture(auto_approve, permission_auto_confirm);
        insert(&tab_id, permission_event());
        assert_eq!(claim(&fixture.service, &tab_id).is_some(), automatic);
        assert_eq!(kind(ACTIVE.lock().get(&tab_id)), if automatic { PromptKind::AutomaticPermission } else { PromptKind::Permission });
        let unrelated = serde_json::from_value(json!({"question":"opaque","options":[{"label":"Once","value":"once"}]})).unwrap();
        assert_eq!(super::super::kind_for_prompt(&tab_id, Some(&unrelated)), PromptKind::Structured);
        assert!(claim(&fixture.service, &tab_id).is_none());
        super::super::clear_tab(&tab_id);
        super::super::register_endpoint(&tab_id, "http://127.0.0.1:1".into(), "/work".into());
        super::super::bind_launch(&tab_id, "own-nonce");
        insert(&tab_id, json!({"type":"question.asked","properties":{"id":"question","sessionID":"own-session","questions":[{"question":"opaque","options":[{"label":"Once"},{"label":"Always"},{"label":"Reject"}]}]}}));
        assert!(claim(&fixture.service, &tab_id).is_none());
        assert_eq!(kind(ACTIVE.lock().get(&tab_id)), PromptKind::Structured);
        super::super::clear_tab(&tab_id);
    }
}

#[test]
fn automatic_permission_has_one_writer_across_manual_and_repeated_event_races() {
    let (fixture, tab_id) = fixture(true, true);
    insert(&tab_id, permission_event());
    let reply = claim(&fixture.service, &tab_id).unwrap();
    assert_eq!(fixture.service.try_permission_auto_confirm(&tab_id, None), crate::service::permission_auto_confirm::Verdict::NotApplicable);
    let prompt = super::super::pending_for_tab(&tab_id).unwrap();
    fixture.service.apply_prompt_decision_with_kind(
        &crate::agent::prompt::PromptDecision::NewPrompt { tab_id: tab_id.clone(), prompt },
        None,
        PromptKind::Structured,
        false,
    );
    assert_eq!(fixture.service.mux.read().get_tab(&tab_id).unwrap().pending_prompt_kind, Some(PromptKind::AutomaticPermission));
    assert!(fixture.ui.0.lock().iter().filter(|(name, _)| name == "prompt-state-changed").all(|(_, value)| value["kind"] == "automatic_permission"));
    assert!(super::super::respond(&fixture.service, &tab_id, "once").unwrap().is_err());
    assert!(claim(&fixture.service, &tab_id).is_none());
    let mut writes = 0;
    reply.run_with(&fixture.service, &tab_id, |_, route| {
        assert_eq!(route.request_id, "own-request");
        writes += 1;
        Ok(())
    });
    insert(&tab_id, permission_event());
    assert!(claim(&fixture.service, &tab_id).is_none());
    assert_eq!(kind(ACTIVE.lock().get(&tab_id)), PromptKind::AutomaticPermission);
    assert_eq!(writes, 1);
    super::super::clear_tab(&tab_id);
}

#[test]
fn automatic_permission_failure_surfaces_manual_request_without_replay() {
    let (fixture, tab_id) = fixture(true, false);
    insert(&tab_id, permission_event());
    claim(&fixture.service, &tab_id).unwrap().run_with(&fixture.service, &tab_id, |_, _| Err("response uncertain".into()));
    assert_eq!(kind(ACTIVE.lock().get(&tab_id)), PromptKind::Permission);
    assert!(claim(&fixture.service, &tab_id).is_none());
    insert(&tab_id, permission_event());
    assert!(claim(&fixture.service, &tab_id).is_none());
    assert_eq!(kind(ACTIVE.lock().get(&tab_id)), PromptKind::Permission);
    super::super::clear_tab(&tab_id);
}

#[test]
fn automatic_permission_revalidates_launch_binding_and_mode_before_the_writer() {
    for change_mode in [false, true] {
        let (fixture, tab_id) = fixture(true, false);
        insert(&tab_id, permission_event());
        let reply = claim(&fixture.service, &tab_id).unwrap();
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            if change_mode { tab.auto_approve = false; } else { tab.session_binding_nonce = Some("replacement".into()); }
        }
        reply.run_with(&fixture.service, &tab_id, |_, _| panic!("stale request reached writer"));
        assert!(claim(&fixture.service, &tab_id).is_none());
        assert_eq!(kind(ACTIVE.lock().get(&tab_id)), if change_mode { PromptKind::Permission } else { PromptKind::Structured });
        super::super::clear_tab(&tab_id);
    }
}
