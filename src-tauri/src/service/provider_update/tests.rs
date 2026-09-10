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

/// Source tab lives in the ordinary first workspace; General is a separate
/// protected workspace, so "raised in General" is a real assertion.
fn fixture(
    audience: TabAudience,
) -> (
    ServiceFixture,
    String,
    std::sync::mpsc::Receiver<ProviderUpdateTestEffect>,
) {
    let (fixture, tab_id, _, receiver) = general_fixture(audience);
    (fixture, tab_id, receiver)
}

fn general_fixture(
    audience: TabAudience,
) -> (
    ServiceFixture,
    String,
    String,
    std::sync::mpsc::Receiver<ProviderUpdateTestEffect>,
) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some("pty-update".to_string());
    tab.agent_type = Some("provider-fixture".to_string());
    tab.audience = audience;
    crate::service::general_agent::ensure_general_workspace(&mut mux, false);
    let general_workspace =
        crate::service::general_agent::general_workspace_id(&mux).expect("General workspace");
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_update_command(
            "provider-fixture",
            "provider-fixture update",
        ),
    ));
    let (sender, receiver) = std::sync::mpsc::channel();
    fixture.service.provider_updates.set_probe(sender);
    (fixture, tab_id, general_workspace, receiver)
}

/// The single live prompt, read from state rather than awaited on the channel.
fn sole_prompt(service: &ServiceContext) -> ProviderUpdatePromptView {
    let mut prompts = service.list_provider_update_prompts();
    assert_eq!(prompts.len(), 1, "expected exactly one update prompt");
    prompts.remove(0)
}

/// `(tab_id, pty_id)` of the tab the press raised.
fn update_tab(service: &ServiceContext, prompt_id: &str) -> (String, String) {
    let tab_id = service
        .provider_updates
        .get(prompt_id)
        .and_then(|entry| entry.runner_tab_id)
        .expect("update tab");
    let pty_id = service
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.pty_id.clone())
        .expect("update tab pty");
    (tab_id, pty_id)
}

fn tabs_in(
    snapshot: &crate::multiplexer::state::MultiplexerSnapshot,
    workspace_id: &str,
) -> Vec<String> {
    snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.id == workspace_id)
        .map(|workspace| workspace.tabs.clone())
        .unwrap_or_default()
}

fn handle_prompt(
    service: &ServiceContext,
    tab_id: &str,
    completion: bool,
) -> ProviderUpdatePromptOutcome {
    service.handle_provider_update_prompt(
        tab_id,
        "provider-fixture",
        capability(completion),
        vec![b"install".to_vec()],
        vec![b"decline".to_vec()],
    )
}

fn await_effect(
    receiver: &std::sync::mpsc::Receiver<ProviderUpdateTestEffect>,
    matches: impl Fn(&ProviderUpdateTestEffect) -> bool,
) -> ProviderUpdateTestEffect {
    loop {
        let effect = receiver
            .recv_timeout(Duration::from_secs(10))
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
        update_command: "provider-fixture update".into(),
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
    assert!(registry.bind_runner(&prompt_id, "runner-tab".into()));
    assert!(registry
        .claim_completion("runner-tab", "runner-pty")
        .is_none());
    assert!(registry
        .claim_runner_dispatch(&prompt_id, "runner-tab", "runner-pty")
        .is_some());
    assert!(registry
        .claim_completion("runner-tab", "runner-pty")
        .is_some());
    assert!(registry
        .claim_completion("runner-tab", "runner-pty")
        .is_none());
}

#[test]
fn update_tab_spec_declares_a_visible_user_tab_in_the_given_workspace() {
    let entry = PendingProviderUpdate {
        view: ProviderUpdatePromptView {
            prompt_id: "prompt-spec".into(),
            tab_id: "source-tab".into(),
            provider_name: "Provider".into(),
            restart_required: true,
            status: ProviderUpdateStatus::Installing,
            message: "Installing".into(),
        },
        provider_id: "provider-fixture".into(),
        pty_id: "source-pty".into(),
        capability: capability(true),
        update_command: "provider-fixture update".into(),
        decline_writes: Vec::new(),
        consent: None,
        phase: ProviderUpdatePhase::Installing,
        waiter_started: false,
        runner_tab_id: None,
        runner_pty_id: None,
        install_submitted: false,
    };

    let spec = provider_update_tab_spec(&entry, "ws-general");
    assert_eq!(spec.workspace_id.as_deref(), Some("ws-general"));
    assert_eq!(
        spec.visibility,
        crate::service::spawn::TabVisibility::Visible
    );
    assert_eq!(spec.audience, TabAudience::User);
    assert_eq!(
        spec.description.as_deref(),
        Some(provider_update_tab_description("prompt-spec").as_str())
    );
    assert!(matches!(
        spec.command,
        crate::service::spawn::TabCommand::Shell
    ));
    assert!(spec.eager);
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn update_tab_lands_in_the_general_workspace_and_takes_focus() {
    let (fixture, tab_id, general_workspace, receiver) = general_fixture(TabAudience::User);
    let source_workspace = fixture
        .service
        .mux
        .read()
        .find_workspace_for_tab(&tab_id)
        .unwrap();
    assert_ne!(source_workspace, general_workspace);

    handle_prompt(&fixture.service, &tab_id, true);
    let prompt = await_status(&receiver, ProviderUpdateStatus::AwaitingConsent);
    fixture
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, true)
        .unwrap();
    await_status(&receiver, ProviderUpdateStatus::Installing);

    let (update_tab_id, _) = update_tab(&fixture.service, &prompt.prompt_id);
    let snapshot = fixture.service.mux.read().snapshot();
    assert!(tabs_in(&snapshot, &general_workspace).contains(&update_tab_id));
    assert!(!tabs_in(&snapshot, &source_workspace).contains(&update_tab_id));
    assert_eq!(
        snapshot.focused_tab_id.as_deref(),
        Some(update_tab_id.as_str())
    );
    assert_eq!(
        snapshot.active_workspace_id.as_deref(),
        Some(general_workspace.as_str())
    );
    assert_eq!(
        snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.id == general_workspace)
            .and_then(|workspace| workspace.active_tab_id.as_deref()),
        Some(update_tab_id.as_str())
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn update_tab_is_a_user_visible_tab_not_an_automation_surface() {
    let (fixture, tab_id, general_workspace, receiver) = general_fixture(TabAudience::User);
    handle_prompt(&fixture.service, &tab_id, true);
    let prompt = await_status(&receiver, ProviderUpdateStatus::AwaitingConsent);
    fixture
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, true)
        .unwrap();
    await_status(&receiver, ProviderUpdateStatus::Installing);

    let (update_tab_id, _) = update_tab(&fixture.service, &prompt.prompt_id);
    let snapshot = fixture.service.mux.read().snapshot();
    let tab = snapshot
        .tabs
        .get(&update_tab_id)
        .expect("update tab in state");
    assert!(!tab.hidden);
    assert!(tab.show_in_tabbar);
    assert_eq!(tab.audience, TabAudience::User);
    assert_eq!(tab.agent_type, None);
    assert!(tabs_in(&snapshot, &general_workspace).contains(&update_tab_id));
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn a_second_press_focuses_the_live_update_tab_instead_of_duplicating_it() {
    let (fixture, tab_id, general_workspace, receiver) = general_fixture(TabAudience::User);
    handle_prompt(&fixture.service, &tab_id, true);
    let prompt = await_status(&receiver, ProviderUpdateStatus::AwaitingConsent);
    fixture
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, true)
        .unwrap();
    await_status(&receiver, ProviderUpdateStatus::Installing);
    await_effect(
        &receiver,
        |effect| matches!(effect, ProviderUpdateTestEffect::Wrote { bytes, .. } if bytes == b"provider-fixture update"),
    );

    let (update_tab_id, _) = update_tab(&fixture.service, &prompt.prompt_id);
    let before = tabs_in(&fixture.service.mux.read().snapshot(), &general_workspace);
    fixture
        .service
        .mux
        .write()
        .set_focused_tab(&tab_id)
        .unwrap();

    fixture
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, true)
        .unwrap();

    let snapshot = fixture.service.mux.read().snapshot();
    assert_eq!(tabs_in(&snapshot, &general_workspace), before);
    assert_eq!(
        snapshot.focused_tab_id.as_deref(),
        Some(update_tab_id.as_str())
    );
    assert_eq!(
        update_tab(&fixture.service, &prompt.prompt_id).0,
        update_tab_id
    );
    assert_eq!(
        receiver
            .try_iter()
            .filter(|effect| matches!(
                effect,
                ProviderUpdateTestEffect::Wrote { bytes, .. } if bytes == b"provider-fixture update"
            ))
            .count(),
        0,
        "a repeat press must not re-authorize an install"
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn a_failed_update_tab_stays_visible_and_is_never_reported_as_success() {
    let (fixture, tab_id, general_workspace, receiver) = general_fixture(TabAudience::User);
    handle_prompt(&fixture.service, &tab_id, true);
    let prompt = await_status(&receiver, ProviderUpdateStatus::AwaitingConsent);
    fixture
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, true)
        .unwrap();
    await_status(&receiver, ProviderUpdateStatus::Installing);
    let (update_tab_id, update_pty_id) = update_tab(&fixture.service, &prompt.prompt_id);
    let notifications_before = fixture.service.notif.lock().list(false).len();

    let entry = fixture
        .service
        .provider_updates
        .get(&prompt.prompt_id)
        .unwrap();
    fixture
        .service
        .fail_provider_update(entry, "the update command exited non-zero");

    let failed = await_status(&receiver, ProviderUpdateStatus::Failed);
    assert_eq!(failed.prompt_id, prompt.prompt_id);
    let snapshot = fixture.service.mux.read().snapshot();
    let tab = snapshot
        .tabs
        .get(&update_tab_id)
        .expect("failed update stays readable in its own tab");
    assert!(!tab.hidden);
    assert!(tabs_in(&snapshot, &general_workspace).contains(&update_tab_id));
    assert_eq!(
        fixture.service.notif.lock().list(false).len(),
        notifications_before,
        "a failed update is never swallowed into a notification"
    );

    // A late completion signal must not overwrite the recorded failure.
    fixture
        .service
        .handle_provider_update_completed(&update_tab_id, &update_pty_id);
    let statuses: Vec<_> = fixture
        .service
        .list_provider_update_prompts()
        .into_iter()
        .filter(|view| view.prompt_id == prompt.prompt_id)
        .map(|view| view.status)
        .collect();
    assert_eq!(statuses, vec![ProviderUpdateStatus::Failed]);
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn closing_the_update_terminal_after_dispatch_completes_and_frees_the_source_tab() {
    // Read derived state directly because an effect channel cannot distinguish a missed event from an incorrect event.
    let (fixture, tab_id, _general_workspace, _probe) = general_fixture(TabAudience::User);
    handle_prompt(&fixture.service, &tab_id, true);
    let prompt = sole_prompt(&fixture.service);
    assert_eq!(prompt.status, ProviderUpdateStatus::AwaitingConsent);

    fixture
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, true)
        .unwrap();
    assert_eq!(
        sole_prompt(&fixture.service).status,
        ProviderUpdateStatus::Installing
    );
    let (update_tab_id, _) = update_tab(&fixture.service, &prompt.prompt_id);

    fixture
        .service
        .mux
        .write()
        .close_tab(&update_tab_id)
        .unwrap();

    // 1. Did reconciliation fire at all, and on which prompt?
    let terminated = fixture.service.reconcile_closed_update_tabs();
    assert_eq!(
        terminated,
        vec![prompt.prompt_id.clone()],
        "a closed update tab must terminate exactly its own prompt"
    );

    // 2. Closing after the shell command was dispatched is the visible runner's completion event.
    let closed: Vec<_> = fixture
        .service
        .list_provider_update_prompts()
        .into_iter()
        .filter(|view| view.prompt_id == prompt.prompt_id)
        .map(|view| view.status)
        .collect();
    assert_eq!(closed, vec![ProviderUpdateStatus::Restarted]);

    // 3. Is the source tab actually free again? The outcome names the branch
    //    taken, so a swallowed prompt reports which gate swallowed it.
    assert_eq!(
        handle_prompt(&fixture.service, &tab_id, true),
        ProviderUpdatePromptOutcome::Raised(ProviderUpdateStatus::AwaitingConsent),
        "the freed source tab must raise a new consent prompt"
    );
    let fresh: Vec<_> = fixture
        .service
        .list_provider_update_prompts()
        .into_iter()
        .filter(|view| view.prompt_id != prompt.prompt_id)
        .collect();
    assert_eq!(
        fresh.len(),
        1,
        "the freed source tab must raise exactly one new prompt"
    );
    assert_eq!(fresh[0].status, ProviderUpdateStatus::AwaitingConsent);
    assert_eq!(fresh[0].tab_id, tab_id);
}

/// The seam with N2: a DECLINE must stay latched (that is the hammering N2
/// stopped), while a FAILED attempt must re-arm, or one closed update tab would
/// silence that provider until it somehow installed itself.
#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn a_declined_update_tab_stays_silent_while_a_failed_one_can_announce_again() {
    let (declined, declined_tab, _ws, _probe) = general_fixture(TabAudience::User);
    handle_prompt(&declined.service, &declined_tab, true);
    let prompt = sole_prompt(&declined.service);
    declined
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, false)
        .unwrap();
    assert_eq!(
        sole_prompt(&declined.service).status,
        ProviderUpdateStatus::Declined
    );
    assert_eq!(
        handle_prompt(&declined.service, &declined_tab, true),
        ProviderUpdatePromptOutcome::ConditionAlreadyAnnounced,
        "a declined update must not be announced again"
    );

    let (failed, failed_tab, _ws, _probe) = general_fixture(TabAudience::User);
    handle_prompt(&failed.service, &failed_tab, true);
    let prompt = sole_prompt(&failed.service);
    failed
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, true)
        .unwrap();
    let entry = failed
        .service
        .provider_updates
        .get(&prompt.prompt_id)
        .unwrap();
    failed
        .service
        .fail_provider_update(entry, "the update command exited non-zero");
    assert_eq!(
        handle_prompt(&failed.service, &failed_tab, true),
        ProviderUpdatePromptOutcome::Raised(ProviderUpdateStatus::AwaitingConsent),
        "a failed update leaves the condition unresolved, so it is news again"
    );
}

/// No Tauri app, no channel, no fixture: proves the state transition the
/// closed-tab path depends on, so a failure here is the mechanism and a failure
/// only in the fixture test above is the harness.
#[test]
fn update_tab_termination_frees_the_source_tab_for_a_new_prompt() {
    let registry = ProviderUpdateRegistry::default();
    let prompt_id = "prompt-close".to_string();
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
        update_command: "provider-fixture update".into(),
        decline_writes: Vec::new(),
        consent: Some(RecordedConsent {
            prompt_id: prompt_id.clone(),
        }),
        phase: ProviderUpdatePhase::Installing,
        waiter_started: false,
        runner_tab_id: Some("update-tab".into()),
        runner_pty_id: Some("update-pty".into()),
        install_submitted: true,
    });

    assert_eq!(
        registry.open_runner_tabs(),
        vec![(prompt_id.clone(), "update-tab".to_string())],
        "a live update tab is what reconciliation looks up"
    );
    assert!(
        registry.has_open_source("source-tab", "provider-fixture"),
        "a live update pins its source tab"
    );

    let entry = registry.get(&prompt_id).unwrap();
    registry.finish(
        entry,
        ProviderUpdateStatus::Failed,
        "the update tab was closed".into(),
    );

    assert!(registry.open_runner_tabs().is_empty());
    assert!(
        !registry.has_open_source("source-tab", "provider-fixture"),
        "a terminated update frees its source tab to prompt again"
    );
    assert!(
        registry.runner_tab(&prompt_id).is_none(),
        "a terminated prompt has no tab left to focus"
    );
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
        ProviderUpdateTestEffect::Wrote { bytes, .. } if bytes == b"provider-fixture update"
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
            |effect| matches!(effect, ProviderUpdateTestEffect::Wrote { bytes, .. } if bytes == b"provider-fixture update"),
        );
        assert!(!receiver
            .try_iter()
            .any(|effect| matches!(effect, ProviderUpdateTestEffect::Restarted { .. })));

        let (update_tab_id, update_pty_id) = update_tab(&fixture.service, &prompt.prompt_id);
        fixture
            .service
            .handle_provider_update_completed(&update_tab_id, &update_pty_id);
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
fn shell_update_runner_does_not_require_an_in_agent_completion_signal() {
    let (fixture, tab_id, receiver) = fixture(TabAudience::User);
    handle_prompt(&fixture.service, &tab_id, false);
    let prompt = await_status(&receiver, ProviderUpdateStatus::AwaitingConsent);
    fixture
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, true)
        .unwrap();
    await_status(&receiver, ProviderUpdateStatus::Installing);
    await_effect(
        &receiver,
        |effect| matches!(effect, ProviderUpdateTestEffect::Wrote { bytes, .. } if bytes == b"provider-fixture update"),
    );
    let (update_tab_id, _) = update_tab(&fixture.service, &prompt.prompt_id);
    fixture
        .service
        .mux
        .write()
        .close_tab(&update_tab_id)
        .unwrap();
    fixture.service.reconcile_closed_update_tabs();
    await_status(&receiver, ProviderUpdateStatus::Restarted);
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
    // The loser either lost the claim outright or landed on the tab the winner
    // raised; neither outcome may authorize a second install.
    assert!(results.iter().filter(|result| result.is_err()).count() <= 1);
    // A replay after the tab exists focuses it instead of failing.
    fixture
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, true)
        .unwrap();
    let effects: Vec<_> = receiver.try_iter().collect();
    assert_eq!(
        effects
            .iter()
            .filter(|effect| matches!(effect, ProviderUpdateTestEffect::Wrote { bytes, .. } if bytes == b"provider-fixture update"))
            .count(),
        1
    );

    let (update_tab_id, update_pty_id) = update_tab(&fixture.service, &prompt.prompt_id);
    let description = provider_update_tab_description(&prompt.prompt_id);
    let raised = fixture
        .service
        .mux
        .read()
        .snapshot()
        .tabs
        .values()
        .filter(|tab| tab.description.as_deref() == Some(description.as_str()))
        .count();
    assert_eq!(raised, 1, "one press raises exactly one update tab");
    fixture
        .service
        .handle_provider_update_completed(&update_tab_id, &update_pty_id);
    fixture
        .service
        .handle_provider_update_completed(&update_tab_id, &update_pty_id);
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
            ProviderUpdateTestEffect::Wrote { bytes, .. } if bytes == b"provider-fixture update"
        )));
    }
}
/// A consented prompt parked in `Deferred` by an unresolved delivery, plus the
/// waiter that consent started.
fn deferred_prompt(
    fixture: &ServiceFixture,
    tab_id: &str,
    receiver: &std::sync::mpsc::Receiver<ProviderUpdateTestEffect>,
) -> String {
    park_delivery(fixture, tab_id);
    handle_prompt(&fixture.service, tab_id, true);
    let prompt = await_status(receiver, ProviderUpdateStatus::AwaitingConsent);
    fixture
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, true)
        .unwrap();
    await_status(receiver, ProviderUpdateStatus::Deferred);
    prompt.prompt_id
}

fn park_delivery(fixture: &ServiceFixture, tab_id: &str) {
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(tab_id)
        .unwrap()
        .pending_delivery = Some(pending_delivery(PendingDeliveryState::Pending, false));
}

fn resolve_delivery(fixture: &ServiceFixture, tab_id: &str) {
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(tab_id)
        .unwrap()
        .pending_delivery
        .as_mut()
        .unwrap()
        .confirm_strong()
        .unwrap();
}

fn is_update_write(effect: &ProviderUpdateTestEffect) -> bool {
    matches!(effect, ProviderUpdateTestEffect::Wrote { bytes, .. } if bytes == b"provider-fixture update")
}

fn is_installing(effect: &ProviderUpdateTestEffect) -> bool {
    matches!(effect, ProviderUpdateTestEffect::Emitted(view) if view.status == ProviderUpdateStatus::Installing)
}

fn is_waiter_finished(effect: &ProviderUpdateTestEffect) -> bool {
    matches!(effect, ProviderUpdateTestEffect::WaiterFinished { .. })
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn deferred_provider_update_waiter_exits_on_teardown() {
    let (fixture, tab_id, receiver) = fixture(TabAudience::User);
    park_delivery(&fixture, &tab_id);
    handle_prompt(&fixture.service, &tab_id, true);
    let prompt = await_status(&receiver, ProviderUpdateStatus::AwaitingConsent);
    assert_eq!(
        fixture.service.provider_updates.waiter_count(),
        0,
        "an unconsented prompt owns no waiter"
    );

    fixture
        .service
        .respond_provider_update_prompt(&prompt.prompt_id, true)
        .unwrap();
    await_status(&receiver, ProviderUpdateStatus::Deferred);
    assert_eq!(
        fixture.service.provider_updates.waiter_count(),
        1,
        "a deferred prompt owns exactly one registered waiter"
    );
    // No bus frame has been published, so the parked waiter cannot be inside a
    // recheck; the sole strong owner proves it holds only a Weak context.
    assert_eq!(
        fixture
            .service
            .provider_updates
            .execution_owner_strong_count(),
        1,
        "a parked waiter holds no strong execution context"
    );

    // The delivery never resolves, so only owner teardown ends this waiter; the
    // probe channel outlives the dropped fixture and reports the exit.
    drop(fixture);
    let finished = await_effect(&receiver, is_waiter_finished);
    assert!(matches!(
        &finished,
        ProviderUpdateTestEffect::WaiterFinished { prompt_id } if prompt_id == &prompt.prompt_id
    ));
    assert!(
        matches!(
            receiver.recv_timeout(Duration::from_secs(10)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        ),
        "no waiter or service clone may outlive the dropped fixture"
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn provider_update_resolution_executes_exactly_once() {
    let (fixture, tab_id, receiver) = fixture(TabAudience::User);
    let prompt_id = deferred_prompt(&fixture, &tab_id, &receiver);

    // A relevant event on an unresolved prompt proves the waiter is live and
    // rechecking, so a later duplicate cannot be excused by a dead thread.
    fixture
        .service
        .events
        .publish(crate::api::events::SystemEvent::MuxStateChanged);
    await_status(&receiver, ProviderUpdateStatus::Deferred);

    resolve_delivery(&fixture, &tab_id);
    for _ in 0..4 {
        fixture
            .service
            .events
            .publish(crate::api::events::SystemEvent::MuxStateChanged);
    }
    await_status(&receiver, ProviderUpdateStatus::Installing);
    await_effect(&receiver, is_update_write);
    await_effect(&receiver, is_waiter_finished);
    assert_eq!(
        fixture.service.provider_updates.waiter_count(),
        0,
        "a naturally completed waiter retires its own record"
    );

    let later: Vec<_> = receiver.try_iter().collect();
    assert!(
        !later.iter().any(is_installing),
        "duplicate events may not repeat the Installing transition"
    );
    assert!(
        !later.iter().any(is_update_write),
        "duplicate events may not repeat the update execution"
    );
    let settled = sole_prompt(&fixture.service);
    assert_eq!(settled.prompt_id, prompt_id);
    assert_eq!(settled.status, ProviderUpdateStatus::Installing);
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn lagged_provider_update_waiter_rechecks_authoritative_state() {
    let (fixture, tab_id, receiver) = fixture(TabAudience::User);
    park_delivery(&fixture, &tab_id);
    handle_prompt(&fixture.service, &tab_id, true);
    let prompt = await_status(&receiver, ProviderUpdateStatus::AwaitingConsent);
    // Consent through the state machine rather than the public responder, so
    // this test owns the exact wake stream the waiter is handed.
    let principals = std::collections::HashMap::from([(
        prompt.prompt_id.clone(),
        ConsentPrincipal::LocalInteractive,
    )]);
    let claimed = fixture
        .service
        .provider_updates
        .claim_provider_responses(&prompt.prompt_id, true, &principals)
        .unwrap();
    assert!(matches!(claimed.as_slice(), [ClaimedResponse::Accept(_)]));
    assert!(!fixture
        .service
        .try_execute_provider_update(&prompt.prompt_id)
        .unwrap());
    await_status(&receiver, ProviderUpdateStatus::Deferred);

    resolve_delivery(&fixture, &tab_id);
    let lagged = fixture.service.events.subscribe();
    fixture
        .service
        .events
        .publish(crate::api::events::SystemEvent::MuxStateChanged);
    for index in 0..(crate::api::events::SYSTEM_BUS_CAPACITY * 2) {
        fixture
            .service
            .events
            .publish(crate::api::events::SystemEvent::TabFocused {
                tab_id: format!("filler-{index}"),
            });
    }

    assert!(fixture
        .service
        .start_provider_update_waiter(prompt.prompt_id.clone(), lagged));
    await_status(&receiver, ProviderUpdateStatus::Installing);
    await_effect(&receiver, is_update_write);
    await_effect(&receiver, is_waiter_finished);
    assert_eq!(fixture.service.provider_updates.waiter_count(), 0);
}

#[test]
#[cfg_attr(not(windows), ignore = "mock Tauri desktop fixture")]
fn provider_update_waiter_cannot_register_after_shutdown_begins() {
    let (fixture, tab_id, receiver) = fixture(TabAudience::User);
    let prompt_id = deferred_prompt(&fixture, &tab_id, &receiver);
    assert_eq!(fixture.service.provider_updates.waiter_count(), 1);

    // Registration is gated on a channel the test releases only after shutdown
    // has run to completion, pinning the old spawn-then-register interleaving.
    let (release, held) = std::sync::mpsc::channel::<()>();
    let (started, outcome) = std::sync::mpsc::channel::<bool>();
    let service = fixture.service.clone();
    let late_stream = fixture.service.events.subscribe();
    let late_prompt = prompt_id.clone();
    let racer = std::thread::spawn(move || {
        held.recv().unwrap();
        started
            .send(service.start_provider_update_waiter(late_prompt, late_stream))
            .unwrap();
    });

    assert_eq!(fixture.service.shutdown_provider_update_waiters(), 1);
    await_effect(&receiver, is_waiter_finished);
    release.send(()).unwrap();
    assert!(
        !outcome.recv_timeout(Duration::from_secs(10)).unwrap(),
        "no waiter may start once shutdown has begun"
    );
    assert_eq!(fixture.service.provider_updates.waiter_count(), 0);
    assert_eq!(fixture.service.shutdown_provider_update_waiters(), 0);
    racer.join().unwrap();
}
