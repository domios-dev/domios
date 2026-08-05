use std::sync::{Arc, Barrier};
use std::time::Duration;

use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{
    DeliveryAttemptProvenance, PendingDeliveryChannel, PendingDeliveryRecord,
};
use crate::service::test_fixture::ServiceFixture;

fn capability(completion: bool) -> Option<ProviderSelfUpdateCapability> {
    Some(ProviderSelfUpdateCapability {
        in_session: true,
        restart_required: true,
        completion_signal: completion
            .then_some(ProviderUpdateCompletionSignal::ProviderClassification),
    })
}

fn fixture(
    audience: TabAudience,
) -> (
    ServiceFixture,
    String,
    std::sync::mpsc::Receiver<ProviderUpdateTestEffect>,
) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some("pty-update".to_string());
    tab.agent_type = Some("provider-fixture".to_string());
    tab.audience = audience;
    let fixture = ServiceFixture::new(mux);
    let (sender, receiver) = std::sync::mpsc::channel();
    fixture.service.provider_updates.set_probe(sender);
    (fixture, tab_id, receiver)
}

fn handle_prompt(service: &ServiceContext, tab_id: &str, completion: bool) {
    service.handle_provider_update_prompt(
        tab_id,
        "provider-fixture",
        capability(completion),
        vec![b"install".to_vec()],
        vec![b"decline".to_vec()],
    );
}

fn await_effect(
    receiver: &std::sync::mpsc::Receiver<ProviderUpdateTestEffect>,
    matches: impl Fn(&ProviderUpdateTestEffect) -> bool,
) -> ProviderUpdateTestEffect {
    loop {
        let effect = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("provider update effect deadline");
        if matches(&effect) {
            return effect;
        }
    }
}

fn await_status(
    receiver: &std::sync::mpsc::Receiver<ProviderUpdateTestEffect>,
    status: ProviderUpdateStatus,
) -> ProviderUpdatePromptView {
    match await_effect(
        receiver,
        |effect| matches!(effect, ProviderUpdateTestEffect::Emitted(view) if view.status == status),
    ) {
        ProviderUpdateTestEffect::Emitted(view) => view,
        _ => unreachable!(),
    }
}

fn pending_delivery(state: PendingDeliveryState, active_attempt: bool) -> PendingDeliveryRecord {
    let mut record = PendingDeliveryRecord::new(
        "digest".to_string(),
        PendingDeliveryChannel::InteractiveInput,
    );
    if active_attempt {
        record
            .begin_attempt(DeliveryAttemptProvenance {
                attempt_id: "attempt".to_string(),
                started_at_ms: 1,
            })
            .unwrap();
    }
    match state {
        PendingDeliveryState::Pending => {}
        PendingDeliveryState::Ingested => record.mark_ingested(),
        PendingDeliveryState::Unconfirmed => record.mark_unconfirmed(),
        PendingDeliveryState::AcceptedStrong => record.confirm_strong().unwrap(),
        PendingDeliveryState::AcceptedWeak => record.confirm().unwrap(),
    }
    record
}

#[test]
fn runner_completion_requires_an_install_submission_and_is_claimed_once() {
    let registry = ProviderUpdateRegistry::default();
    let prompt_id = "prompt-runner".to_string();
    registry.insert(PendingProviderUpdate {
        view: ProviderUpdatePromptView {
            prompt_id: prompt_id.clone(),
            tab_id: "source-tab".into(),
            provider_name: "Provider".into(),
            restart_required: true,
            status: ProviderUpdateStatus::Installing,
            message: "Installing".into(),
        },
        provider_id: "provider-fixture".into(),
        pty_id: "source-pty".into(),
        capability: capability(true),
        install_writes: Vec::new(),
        decline_writes: Vec::new(),
        consent: Some(RecordedConsent {
            prompt_id: prompt_id.clone(),
        }),
        phase: ProviderUpdatePhase::Installing,
        waiter_started: false,
        runner_tab_id: None,
        runner_pty_id: None,
        install_submitted: false,
    });
    assert!(registry.bind_runner(&prompt_id, "runner-tab".into(), "runner-pty".into()));
    assert!(registry
        .claim_completion("runner-tab", "runner-pty")
        .is_none());
    assert!(registry
        .claim_runner_prompt(
            &prompt_id,
            "runner-tab",
            "runner-pty",
            vec![b"install".to_vec()],
        )
        .is_some());
    assert!(registry
        .claim_completion("runner-tab", "runner-pty")
        .is_some());
    assert!(registry
        .claim_completion("runner-tab", "runner-pty")
        .is_none());
}

#[test]
fn runner_timeout_is_claimed_once() {
    let registry = ProviderUpdateRegistry::default();
    let prompt_id = "prompt-timeout".to_string();
    registry.insert(PendingProviderUpdate {
        view: ProviderUpdatePromptView {
            prompt_id: prompt_id.clone(),
            tab_id: "source-tab".into(),
            provider_name: "Provider".into(),
            restart_required: true,
            status: ProviderUpdateStatus::Installing,
            message: "Installing".into(),
        },
        provider_id: "provider-fixture".into(),
        pty_id: "source-pty".into(),
        capability: capability(true),
        install_writes: Vec::new(),
        decline_writes: Vec::new(),
        consent: Some(RecordedConsent {
            prompt_id: prompt_id.clone(),
        }),
        phase: ProviderUpdatePhase::Installing,
        waiter_started: false,
        runner_tab_id: Some("runner-tab".into()),
        runner_pty_id: Some("runner-pty".into()),
        install_submitted: true,
    });

    assert!(registry
        .claim_runner_timeout(&prompt_id, "runner-tab")
        .is_some());
    assert!(registry
        .claim_runner_timeout(&prompt_id, "runner-tab")
        .is_none());
    assert!(registry
        .claim_completion("runner-tab", "runner-pty")
        .is_none());
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn unauthorized_provider_update_is_visible_and_declined() {
    let (fixture, tab_id, receiver) = fixture(TabAudience::Automation);
    handle_prompt(&fixture.service, &tab_id, true);

    let view = await_status(&receiver, ProviderUpdateStatus::Declined);
    assert_eq!(view.tab_id, tab_id);
    assert!(view.message.contains("no interactive user audience"));
    assert!(matches!(
        await_effect(
            &receiver,
            |effect| matches!(effect, ProviderUpdateTestEffect::Wrote { pty_id, bytes } if pty_id == "pty-update" && bytes == b"decline")
        ),
        ProviderUpdateTestEffect::Wrote { .. }
    ));
    assert!(!receiver.try_iter().any(|effect| matches!(
        effect,
        ProviderUpdateTestEffect::Wrote { bytes, .. } if bytes == b"install"
    )));
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn terminal_provider_update_is_listed_until_acknowledged() {
    let (fixture, tab_id, receiver) = fixture(TabAudience::Automation);
    handle_prompt(&fixture.service, &tab_id, true);
    let prompt = await_status(&receiver, ProviderUpdateStatus::Declined);

    assert!(fixture
        .service
        .list_provider_update_prompts()
        .iter()
        .any(|view| view.prompt_id == prompt.prompt_id));
    fixture
        .service
        .acknowledge_provider_update_prompt(&prompt.prompt_id)
        .unwrap();
    assert!(!fixture
        .service
        .list_provider_update_prompts()
        .iter()
        .any(|view| view.prompt_id == prompt.prompt_id));
    assert!(fixture
        .service
        .acknowledge_provider_update_prompt(&prompt.prompt_id)
        .is_err());
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn provider_update_acknowledgement_is_exact_and_terminal_only() {
    let (user_fixture, tab_id, receiver) = fixture(TabAudience::User);
    handle_prompt(&user_fixture.service, &tab_id, true);
    let prompt = await_status(&receiver, ProviderUpdateStatus::AwaitingConsent);

    assert!(user_fixture
        .service
        .acknowledge_provider_update_prompt(&prompt.prompt_id)
        .is_err());
    user_fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .pending_delivery = Some(pending_delivery(PendingDeliveryState::Pending, false));
    user_fixture
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, true)
        .unwrap();
    await_status(&receiver, ProviderUpdateStatus::Deferred);
    assert!(user_fixture
        .service
        .acknowledge_provider_update_prompt(&prompt.prompt_id)
        .is_err());

    let (terminal_fixture, terminal_tab_id, terminal_receiver) = fixture(TabAudience::Automation);
    handle_prompt(&terminal_fixture.service, &terminal_tab_id, true);
    let terminal = await_status(&terminal_receiver, ProviderUpdateStatus::Declined);
    assert!(terminal_fixture
        .service
        .acknowledge_provider_update_prompt("different-prompt")
        .is_err());
    terminal_fixture
        .service
        .acknowledge_provider_update_prompt(&terminal.prompt_id)
        .unwrap();
    assert!(terminal_fixture
        .service
        .acknowledge_provider_update_prompt(&terminal.prompt_id)
        .is_err());
    let remaining = user_fixture.service.list_provider_update_prompts();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].prompt_id, prompt.prompt_id);
    assert_eq!(remaining[0].status, ProviderUpdateStatus::Deferred);
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn authorized_provider_update_restarts_after_delivery_resolution() {
    for (state, active_attempt) in [
        (PendingDeliveryState::Pending, false),
        (PendingDeliveryState::Ingested, false),
        (PendingDeliveryState::Unconfirmed, false),
        (PendingDeliveryState::Pending, true),
    ] {
        let (fixture, tab_id, receiver) = fixture(TabAudience::User);
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .pending_delivery = Some(pending_delivery(state, active_attempt));
        handle_prompt(&fixture.service, &tab_id, true);
        let prompt = await_status(&receiver, ProviderUpdateStatus::AwaitingConsent);

        fixture
            .service
            .respond_provider_update_prompt(&prompt.prompt_id, true)
            .unwrap();
        await_status(&receiver, ProviderUpdateStatus::Deferred);
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .pending_delivery
            .as_mut()
            .unwrap()
            .confirm_strong()
            .unwrap();
        fixture
            .service
            .events
            .publish(crate::api::events::SystemEvent::MuxStateChanged);
        await_status(&receiver, ProviderUpdateStatus::Installing);
        await_effect(
            &receiver,
            |effect| matches!(effect, ProviderUpdateTestEffect::Wrote { bytes, .. } if bytes == b"install"),
        );
        assert!(!receiver
            .try_iter()
            .any(|effect| matches!(effect, ProviderUpdateTestEffect::Restarted { .. })));

        fixture
            .service
            .handle_provider_update_completed(&tab_id, "pty-update");
        assert!(matches!(
            await_effect(
                &receiver,
                |effect| matches!(effect, ProviderUpdateTestEffect::Restarted { tab_id: restarted } if restarted == &tab_id)
            ),
            ProviderUpdateTestEffect::Restarted { .. }
        ));
        await_status(&receiver, ProviderUpdateStatus::Restarted);
    }
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn provider_without_completion_signal_never_claims_restart() {
    let (fixture, tab_id, receiver) = fixture(TabAudience::User);
    handle_prompt(&fixture.service, &tab_id, false);
    let prompt = await_status(&receiver, ProviderUpdateStatus::AwaitingConsent);
    fixture
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, true)
        .unwrap();
    let unavailable = await_status(&receiver, ProviderUpdateStatus::CompletionUnavailable);
    assert!(unavailable
        .message
        .contains("does not declare an observable completion signal"));
    assert!(!receiver.try_iter().any(|effect| matches!(
        effect,
        ProviderUpdateTestEffect::Wrote { bytes, .. } if bytes == b"install"
    )));
    assert!(fixture
        .service
        .list_provider_update_prompts()
        .iter()
        .any(|view| view.prompt_id == prompt.prompt_id
            && view.status == ProviderUpdateStatus::CompletionUnavailable));
    fixture
        .service
        .acknowledge_provider_update_prompt(&prompt.prompt_id)
        .unwrap();
    assert!(!fixture
        .service
        .list_provider_update_prompts()
        .iter()
        .any(|view| view.prompt_id == prompt.prompt_id));
    fixture
        .service
        .handle_provider_update_completed(&tab_id, "pty-update");
    assert!(!receiver
        .try_iter()
        .any(|effect| matches!(effect, ProviderUpdateTestEffect::Restarted { .. })));
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn provider_update_concurrent_and_replayed_authority_is_consumed_once() {
    let (fixture, tab_id, receiver) = fixture(TabAudience::User);
    handle_prompt(&fixture.service, &tab_id, true);
    let prompt = await_status(&receiver, ProviderUpdateStatus::AwaitingConsent);
    assert!(fixture
        .service
        .respond_provider_update_prompt("stale-prompt", true)
        .is_err());
    let barrier = Arc::new(Barrier::new(3));
    let calls: Vec<_> = (0..2)
        .map(|_| {
            let service = fixture.service.clone();
            let prompt_id = prompt.prompt_id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                service.respond_provider_update_prompt(&prompt_id, true)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = calls.into_iter().map(|call| call.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    assert!(fixture
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, true)
        .is_err());
    let effects: Vec<_> = receiver.try_iter().collect();
    assert_eq!(
        effects
            .iter()
            .filter(|effect| matches!(effect, ProviderUpdateTestEffect::Wrote { bytes, .. } if bytes == b"install"))
            .count(),
        1
    );

    fixture
        .service
        .handle_provider_update_completed(&tab_id, "pty-update");
    fixture
        .service
        .handle_provider_update_completed(&tab_id, "pty-update");
    let effects: Vec<_> = receiver.try_iter().collect();
    assert_eq!(
        effects
            .iter()
            .filter(|effect| matches!(effect, ProviderUpdateTestEffect::Restarted { .. }))
            .count(),
        1
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn changed_audience_or_mesh_authority_fails_closed() {
    for mesh in [false, true] {
        let (fixture, tab_id, receiver) = fixture(TabAudience::User);
        handle_prompt(&fixture.service, &tab_id, true);
        let prompt = await_status(&receiver, ProviderUpdateStatus::AwaitingConsent);
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            if mesh {
                tab.report_target_mesh_peer = Some("peer".to_string());
            } else {
                tab.audience = TabAudience::Automation;
            }
        }
        fixture
            .service
            .respond_provider_update_prompt(&prompt.prompt_id, true)
            .unwrap();
        await_status(&receiver, ProviderUpdateStatus::Declined);
        await_effect(
            &receiver,
            |effect| matches!(effect, ProviderUpdateTestEffect::Wrote { bytes, .. } if bytes == b"decline"),
        );
        assert!(fixture
            .service
            .respond_provider_update_prompt(&prompt.prompt_id, true)
            .is_err());
        assert!(!receiver.try_iter().any(|effect| matches!(
            effect,
            ProviderUpdateTestEffect::Wrote { bytes, .. } if bytes == b"install"
        )));
    }
}
