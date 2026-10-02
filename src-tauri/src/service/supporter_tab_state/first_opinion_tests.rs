use super::*;
use super::super::{coordinator, identity, privacy, scheduler};
use codeterm_types::decision::DecisionError;
use std::sync::mpsc;

#[test]
fn supporter_first_opinion_secrets_redacted_before_model_state() {
    let secret = "opaque-secret";
    let code = "918273";
    let screen = format!("token={secret}\nAuthorization: Bearer {secret}\nhttps://example.test/login?code={code}&token={secret}\nverification code: {code}");
    let (host, _, seen) = StubHost::new(&screen, StubModel::answering(Ok(choice(OTHER, 0.95))));
    run(&host, &tab(), ObservationTrigger::PromptCandidate).unwrap();
    let serialized = serde_json::to_string(&seen.lock().clone().unwrap()).unwrap();
    assert!(!serialized.contains(secret));
    assert!(!serialized.contains(code));
    let text = std::fs::read_to_string(host._journal_dir.path().join("journal.jsonl")).unwrap();
    assert!(!text.contains(secret));
    assert!(!text.contains(code));
    let bounded = privacy::screen(&"🙂".repeat(5000));
    assert!(bounded.len() <= 8192);
}

#[test]
fn supporter_first_opinion_journal_decided_by_per_arm() {
    for (answer, expected) in [
        (Ok(choice(ERROR_LOOP, 0.60)), "adapter"),
        (Ok(choice(ERROR_LOOP, 0.59)), "fallback(low_confidence)"),
        (Ok(choice(OTHER, 0.99)), "fallback(other)"),
        (Err(DecisionError::Deadline), "fallback(timeout)"),
        (Err(DecisionError::Budget), "fallback(budget)"),
        (Err(DecisionError::Transport("private detail".into())), "fallback(error)"),
    ] {
        let (host, _, _) = StubHost::new("opaque", StubModel::answering(answer));
        run(&host, &tab(), ObservationTrigger::WorkerIdle);
        let text = std::fs::read_to_string(host._journal_dir.path().join("journal.jsonl")).unwrap();
        let decision: Value = text.lines().map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|line| line["action"] == "classification").unwrap();
        assert_eq!(decision["decided_by"], expected);
        assert!(decision["observation_id"].as_u64().unwrap() > 0);
        assert!(!text.contains("private detail"));
    }
    assert_eq!(identity::DecisionSource::Heuristic.as_str(), "heuristic");
    assert_eq!(identity::DecisionSource::NoAdapter.as_str(), "fallback(no_adapter)");
}

#[test]
fn supporter_first_opinion_adapter_disconnect_cancels_result() {
    let registry = Arc::new(SupporterRegistry::new());
    let remove = registry.clone();
    let model = StubModel::answering_with_hook(Ok(choice(ERROR_LOOP, 0.99)), move || remove.unregister("fixture"));
    let (mut host, _, _) = StubHost::new("opaque", model.clone());
    registry.register("fixture", Arc::new(model));
    host.models = registry;
    assert!(run(&host, &tab(), ObservationTrigger::WorkerIdle).is_none());
    assert!(host.deliveries.lock().is_empty());
}

#[test]
fn supporter_first_opinion_observation_ids_unique() {
    let (host, _, _) = StubHost::new("opaque", StubModel::answering(Ok(choice(OTHER, 0.99))));
    run(&host, &tab(), ObservationTrigger::WorkerIdle);
    run(&host, &tab(), ObservationTrigger::WorkerIdle);
    let text = std::fs::read_to_string(host._journal_dir.path().join("journal.jsonl")).unwrap();
    let ids: Vec<_> = text.lines().map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|line| line["action"] == "classification")
        .map(|line| line["observation_id"].as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn supporter_first_opinion_scheduler_bounds_coalesces_and_never_waits_for_model() {
    let queue = scheduler::Scheduler::new();
    let (started_tx, started_rx) = mpsc::channel();
    let mut releases = Vec::new();
    let mut handles = Vec::new();
    for index in 0..4 {
        let (tx, rx) = mpsc::channel();
        releases.push(tx);
        let started = started_tx.clone();
        handles.push(queue.submit(format!("active-{index}"), false, 2, move || {
            started.send(()).unwrap();
            rx.recv().unwrap();
            None
        }).unwrap());
    }
    for _ in 0..4 { started_rx.recv_timeout(Duration::from_secs(2)).unwrap(); }
    let extra = queue.submit("active-0".into(), false, 2, || None).unwrap();
    assert_eq!(queue.status().active, 4);
    assert_eq!(queue.status().queued, 1);
    let replacement = queue.submit("active-0".into(), false, 2, || None).unwrap();
    assert_eq!(queue.status().queued, 1);
    assert!(extra.join().unwrap().is_none());
    for index in 0..31 { queue.submit(format!("queued-{index}"), false, 2, || None).unwrap(); }
    assert_eq!(queue.status().queued, 32);
    assert!(queue.submit("critical".into(), true, 2, || None).is_some());
    assert_eq!(queue.status().queued, 32);
    assert!(queue.submit("oversized".into(), false, 7, || None).is_none());
    for release in releases { release.send(()).unwrap(); }
    for handle in handles { handle.join().unwrap(); }
    replacement.join().unwrap();
    queue.close();
}

#[test]
fn supporter_first_opinion_action_ticket_single_owner_across_ledgers() {
    let owner = coordinator::ActionOwner::default();
    let ledgers = coordinator::LedgerSnapshot::default();
    let first = owner.observe("tab", "nonce-1", Some(11), 22);
    let same = owner.observe("tab", "nonce-1", Some(11), 23);
    assert_eq!(first.occurrence, same.occurrence);
    assert!(!owner.is_current(&first));
    let ticket = owner.claim(&same, ledgers.clone()).unwrap();
    for _ in 0..4 { assert!(owner.claim(&same, ledgers.clone()).is_none()); }
    assert!(owner.ticket_is_current(&ticket, &ledgers));
    let mut responded = ledgers.clone();
    responded.permission = 1;
    assert!(!owner.ticket_is_current(&ticket, &responded));
    owner.observe("tab", "nonce-1", None, 24);
    assert!(!owner.ticket_is_current(&ticket, &ledgers));
    let next = owner.observe("tab", "nonce-1", Some(11), 22);
    assert_ne!(next.occurrence, first.occurrence);
    assert!(owner.claim(&next, ledgers).is_some());
    owner.cancel("tab");
    assert!(!owner.is_current(&next));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn supporter_first_opinion_reused_pty_new_nonce_and_user_answer_rejected() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.link_pty(&tab_id, "reused-pty".into());
    mux.get_tab_mut(&tab_id).unwrap().session_binding_nonce = Some("nonce-1".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let before = identity::LocalIdentity::of_tab(fixture.service.mux.read().get_tab(&tab_id).unwrap());
    fixture.service.mux.write().get_tab_mut(&tab_id).unwrap().session_binding_nonce = Some("nonce-2".into());
    assert!(!fixture.service.observation_identity_is_current(&tab_id, &before));
    let current = identity::LocalIdentity::of_tab(fixture.service.mux.read().get_tab(&tab_id).unwrap());
    assert_ne!(before.launch_key, current.launch_key);
    assert!(fixture.service.observation_identity_is_current(&tab_id, &current));
    fixture.service.mux.write().get_tab_mut(&tab_id).unwrap().agent_session_id = Some("new-session".into());
    assert!(!fixture.service.observation_identity_is_current(&tab_id, &current));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn supporter_first_opinion_service_ledgers_are_part_of_action_ticket() {
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let before = coordinator::LedgerSnapshot::capture(&fixture.service, &tab_id);
    let owner = coordinator::ActionOwner::default();
    let observation = owner.observe(&tab_id, "nonce", Some(11), 22);
    let ticket = owner.claim(&observation, before.clone()).unwrap();
    assert!(crate::service::output::claim_manual_update_occurrence(&tab_id, "occurrence"));
    let after = coordinator::LedgerSnapshot::capture(&fixture.service, &tab_id);
    assert_ne!(before.update, after.update);
    assert!(!owner.ticket_is_current(&ticket, &after));
    crate::service::output::clear_manual_update_occurrence(&tab_id);
}
