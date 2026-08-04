use crate::agent::registry::AgentRegistry;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{
    PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};
use crate::pty::manager::PtyManager;
use crate::service::inject_verify::{InjectionError, InjectionResult};
use codeterm_pty::pty::screen_state::ScreenState;
use codeterm_types::pty::BootStage;
use codeterm_util::lock_hold::{TimedMutex, TimedRwLock};
use parking_lot::Mutex;
use std::cell::Cell;
use std::sync::Arc;
use std::time::Duration;

/// Records what actually reached the PTY for one launch payload and mirrors the
/// transaction's two-phase order: the submit key is written only once ingestion
/// evidence rises, and a turn only counts once the provider acknowledges.
#[derive(Default)]
struct LaunchTransportProbe {
    ingestion_evidence: bool,
    turn_evidence: bool,
    payload_writes: Cell<usize>,
    submit_key_writes: Cell<usize>,
    turns: Cell<usize>,
    queued_chars: Cell<usize>,
}

impl LaunchTransportProbe {
    fn held_paste() -> Self {
        Self::default()
    }

    fn ingesting() -> Self {
        Self {
            ingestion_evidence: true,
            turn_evidence: true,
            ..Self::default()
        }
    }

    fn submit(&self, text: &str) -> Result<InjectionResult, InjectionError> {
        self.payload_writes.set(self.payload_writes.get() + 1);
        if !self.ingestion_evidence {
            self.queued_chars
                .set(self.queued_chars.get() + text.chars().count());
            return Err(InjectionError::EchoMismatch {
                expected_substr: text.chars().take(120).collect(),
                screen_tail: "[Pasted text #1 +47 lines]".into(),
            });
        }
        self.submit_key_writes.set(self.submit_key_writes.get() + 1);
        if !self.turn_evidence {
            return Err(InjectionError::SubmitNeverConfirmed {
                waited: Duration::ZERO,
                screen_tail: String::new(),
            });
        }
        self.turns.set(self.turns.get() + 1);
        Ok(InjectionResult {
            bytes_written: text.len(),
            echo_observed: true,
            submit_observed: true,
            submit_anomaly: false,
            submit_key_writes: 1,
            elapsed: Duration::ZERO,
        })
    }
}

fn reconcile_launch_delivery(
    tab_id: &str,
    pending: &mut PendingDeliveryRecord,
    probe: &LaunchTransportProbe,
    text: &str,
) -> Result<(), crate::service::delivery_error::DeliveryError> {
    let evidence = super::launch_delivery_evidence(tab_id, probe.submit(text));
    crate::service::launch_delivery::reconcile_pending_delivery(tab_id, pending, evidence)
}

/// The reported Claude symptom: the paste renders, so a render window is
/// satisfied, but ingestion and turn evidence never rise.
#[test]
fn held_paste_bootstrap_is_never_confirmed() {
    let probe = LaunchTransportProbe::held_paste();
    let mut pending = PendingDeliveryRecord::new(
        "held-paste".into(),
        PendingDeliveryChannel::InteractiveInput,
    );

    let result = reconcile_launch_delivery(
        "general",
        &mut pending,
        &probe,
        "system contract\n\nbootstrap task",
    );

    assert_eq!(probe.payload_writes.get(), 1, "exactly one payload write");
    assert_eq!(
        probe.submit_key_writes.get(),
        0,
        "a held paste must never take a blind submit key"
    );
    assert_eq!(probe.turns.get(), 0);
    assert_eq!(pending.state(), PendingDeliveryState::Unconfirmed);
    assert_eq!(result.unwrap_err().code(), "submit_unconfirmed");
}

/// The reported Codex symptom: a multiline bootstrap fragmented into several
/// submitted turns with characters left queued.
#[test]
fn multiline_bootstrap_is_one_payload_and_one_turn() {
    let payload = "system contract\n\nbootstrap task\nsecond line";
    let probe = LaunchTransportProbe::ingesting();
    let mut pending =
        PendingDeliveryRecord::new("multiline".into(), PendingDeliveryChannel::InteractiveInput);

    reconcile_launch_delivery("general", &mut pending, &probe, payload).unwrap();

    assert_eq!(probe.payload_writes.get(), 1, "exactly one payload write");
    assert_eq!(
        probe.submit_key_writes.get(),
        1,
        "no per-line submits — one payload takes one submit key"
    );
    assert_eq!(probe.turns.get(), 1, "exactly one turn");
    assert_eq!(probe.queued_chars.get(), 0, "nothing left queued");
    assert_eq!(pending.state(), PendingDeliveryState::AcceptedWeak);
}

/// A written payload whose turn never arrives is uncertainty, not success.
#[test]
fn ingested_paste_without_turn_evidence_stays_unconfirmed() {
    let probe = LaunchTransportProbe {
        ingestion_evidence: true,
        ..LaunchTransportProbe::default()
    };
    let mut pending =
        PendingDeliveryRecord::new("no-turn".into(), PendingDeliveryChannel::InteractiveInput);

    let result = reconcile_launch_delivery("general", &mut pending, &probe, "bootstrap task");

    assert_eq!(probe.turns.get(), 0);
    assert_eq!(pending.state(), PendingDeliveryState::Unconfirmed);
    assert_eq!(result.unwrap_err().code(), "submit_unconfirmed");
}

#[test]
fn observed_submission_is_the_only_source_of_confirmation() {
    let observed = super::launch_delivery_evidence(
        "general",
        Ok(InjectionResult {
            bytes_written: 8,
            echo_observed: true,
            submit_observed: false,
            submit_anomaly: true,
            submit_key_writes: 1,
            elapsed: Duration::ZERO,
        }),
    );

    assert_eq!(
        observed,
        Ok(false),
        "a returned Ok is not evidence — only submit_observed is"
    );
}

#[test]
fn startup_turn_is_delivered_before_session_binding_wait() {
    let steps = std::cell::RefCell::new(Vec::new());
    let binding = super::deliver_before_binding(
        || {
            steps.borrow_mut().push("delivered");
            Ok(())
        },
        || {
            steps.borrow_mut().push("binding");
            "session-id"
        },
    )
    .unwrap();

    assert_eq!(binding, "session-id");
    assert_eq!(*steps.borrow(), ["delivered", "binding"]);
}

#[test]
fn fallback_provider_receives_one_composed_startup_turn() {
    let contract = super::super::instructions::render_system_prompt("codex", None);
    let message =
        super::compose_startup_message(true, contract.clone(), Some("BOOTSTRAP_SENTINEL".into()))
            .unwrap();

    assert_eq!(
        message,
        "-=-codeterm:system context=general-bootstrap-=-BOOTSTRAP_SENTINEL"
    );
    assert_runtime_contract_cardinality(&contract, &message);
    assert_eq!(message.matches("BOOTSTRAP_SENTINEL").count(), 1);
}

#[test]
fn provider_with_native_system_prompt_receives_only_bootstrap_turn() {
    let contract = super::super::instructions::render_system_prompt("codex", None);
    let message =
        super::compose_startup_message(false, contract.clone(), Some("BOOTSTRAP_SENTINEL".into()))
            .unwrap();

    assert_eq!(
        message,
        "-=-codeterm:system context=general-bootstrap-=-BOOTSTRAP_SENTINEL"
    );
    assert_runtime_contract_cardinality(&contract, &message);
    assert_eq!(message.matches("BOOTSTRAP_SENTINEL").count(), 1);
}

fn assert_runtime_contract_cardinality(contract: &str, startup_turn: &str) {
    for sentinel in [
        "You are the General Agent",
        "# CodeTerm CLI reference",
        "# CodeTerm Memory Protocol",
    ] {
        assert_eq!(
            contract.matches(sentinel).count() + startup_turn.matches(sentinel).count(),
            1,
            "runtime contract sentinel must be delivered once: {sentinel}"
        );
    }
}

#[test]
fn provider_without_bootstrap_receives_no_interactive_startup_turn() {
    let contract = super::super::instructions::render_system_prompt("codex", None);

    assert!(super::compose_startup_message(true, contract.clone(), None).is_none());
    assert!(super::compose_startup_message(false, contract, None).is_none());
}

#[test]
fn general_agent_launch_uses_capability_backed_permission_bypass() {
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));

    let permission_bypass = crate::service::permission_policy::authorize_permission_bypass(
        &registry,
        "claude",
        true,
    )
    .is_some();
    let command = super::build_general_agent_resume_command(
        &registry,
        "claude",
        "session-1",
        permission_bypass,
    )
    .expect("resume command");

    assert!(permission_bypass);
    assert!(command.contains("--dangerously-skip-permissions"));
    assert!(crate::service::permission_policy::authorize_permission_bypass(
        &registry,
        "missing-provider",
        true,
    )
    .is_none());
}

fn mux_with_general_tab(stage: Option<BootStage>) -> (MuxState, String) {
    let mut mux = MultiplexerState::empty();
    super::super::workspace::ensure_general_workspace(&mut mux, true);
    let tab_id = mux
        .resolve_tab_id(super::GENERAL_ALIAS)
        .expect("general Tab");
    if let Some(tab) = mux.get_tab_mut(&tab_id) {
        tab.boot_stage = stage;
    }
    (Arc::new(TimedRwLock::new("mux", mux)), tab_id)
}

#[test]
fn general_agent_env_uses_configured_api_port_not_parent_env() {
    let original_port = std::env::var_os("CODETERM_API_PORT");
    let original_url = std::env::var_os("CODETERM_API_URL");
    unsafe {
        std::env::set_var("CODETERM_API_PORT", "7685");
        std::env::set_var("CODETERM_API_URL", "http://127.0.0.1:7685");
    }
    let mut config = crate::config::schema::AppConfig::default();
    config.api.port = 7686;
    config.api.token = Some("test-token".to_string());
    let config: crate::config::ConfigState = Arc::new(Mutex::new(config));

    let env = super::general_agent_env_vars(
        "tab-1",
        "C:/tmp/pad".to_string(),
        "nonce-1".to_string(),
        &config,
    );
    unsafe {
        match original_port {
            Some(p) => std::env::set_var("CODETERM_API_PORT", p),
            None => std::env::remove_var("CODETERM_API_PORT"),
        }
        match original_url {
            Some(u) => std::env::set_var("CODETERM_API_URL", u),
            None => std::env::remove_var("CODETERM_API_URL"),
        }
    }

    assert_eq!(
        env.get("CODETERM_API_PORT").map(String::as_str),
        Some("7686")
    );
    assert_eq!(
        env.get("CODETERM_API_URL").map(String::as_str),
        Some("http://127.0.0.1:7686"),
        "URL must derive from config port, not the inherited prod 7685 env"
    );
    assert_eq!(
        env.get("CODETERM_API_TOKEN").map(String::as_str),
        Some("test-token")
    );
    assert_eq!(
        env.get("CODETERM_MACHINE_AGENT_IDENTITY")
            .map(String::as_str),
        Some("general_agent")
    );
    assert!(!env.contains_key("CODETERM_ROLE"));
}

#[test]
fn tab_scoped_retry_rejects_non_general_tab() {
    let (mux, general_id) = mux_with_general_tab(Some(BootStage::SpawnFailed));

    let err = super::validate_general_agent_tab_retry("other-tab", &mux).unwrap_err();

    assert!(err.contains("other-tab"), "{err}");
    assert!(err.contains(&general_id), "{err}");
}

#[test]
fn tab_scoped_retry_noops_while_launch_is_in_progress() {
    let _serial = super::launch_lock::TEST_SERIAL.lock();
    let (mux, general_id) = mux_with_general_tab(Some(BootStage::PtyStarting));
    let _guard = super::launch_lock::LaunchGuard::acquire().expect("launch guard");

    let should_restart = super::validate_general_agent_tab_retry(&general_id, &mux).unwrap();

    assert!(!should_restart);
}

/// The stuck protected `Main`: latched at a launch stage with no launch
/// alive. Refusing recovery on the stage alone left it unrecoverable through
/// every CLI route.
#[test]
fn tab_scoped_retry_recovers_a_stale_launch_stage() {
    let _serial = super::launch_lock::TEST_SERIAL.lock();
    for stage in [BootStage::ShellReady, BootStage::AgentLaunching] {
        let (mux, general_id) = mux_with_general_tab(Some(stage));

        let should_restart = super::validate_general_agent_tab_retry(&general_id, &mux).unwrap();

        assert!(should_restart, "stale {stage:?} must be recoverable");
    }
}

#[test]
fn tab_scoped_retry_allows_failed_general_tab() {
    let (mux, general_id) = mux_with_general_tab(Some(BootStage::SpawnFailed));

    let should_restart = super::validate_general_agent_tab_retry(&general_id, &mux).unwrap();

    assert!(should_restart);
}

#[test]
fn tab_scoped_retry_claim_is_single_owner() {
    let (mux, general_id) = mux_with_general_tab(Some(BootStage::SpawnFailed));

    let first = super::claim_general_agent_tab_retry(&general_id, &mux).unwrap();
    let second = super::claim_general_agent_tab_retry(&general_id, &mux).unwrap();

    assert!(first);
    assert!(!second);
    let mux = mux.read();
    let tab = mux.get_tab(&general_id).expect("general Tab");
    assert!(tab.agent_booting);
    assert_eq!(tab.boot_stage, Some(BootStage::PtyStarting));
    assert!(tab.boot_failure_reason.is_none());
}

#[test]
fn agent_output_readiness_wakes_on_screen_event() {
    let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
    let registry = Arc::new(AgentRegistry::new());
    registry.register_provider(Box::new(MockProvider::new("agent-marker")));
    let subscription = super::ScreenChangeSubscription::arm(&screen_state, "pty-1");
    let signal = subscription.signal.clone();
    let (armed_tx, armed_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let waiter_screen = screen_state.clone();
    let waiter_registry = registry.clone();
    let waiter = std::thread::spawn(move || {
        let ready = super::await_agent_output_with_signal(
            &waiter_screen,
            &waiter_registry,
            "pty-1",
            &signal,
            Duration::from_secs(1),
            || false,
            || armed_tx.send(()).unwrap(),
        );
        done_tx.send(ready).unwrap();
    });

    armed_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    screen_state.lock().process("pty-1", b"agent-marker ready");

    assert!(done_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    waiter.join().unwrap();
    drop(subscription);
    assert_eq!(screen_state.lock().screen_change_subscriber_count(), 0);
}

#[test]
fn authoritative_binding_is_launch_readiness_evidence() {
    let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
    let registry = AgentRegistry::new();
    let subscription = super::ScreenChangeSubscription::arm(&screen_state, "pty-1");

    assert!(super::await_agent_output_with_signal(
        &screen_state,
        &registry,
        "pty-1",
        &subscription.signal,
        Duration::ZERO,
        || true,
        || panic!("authoritative binding should resolve before waiting"),
    ));
}

#[test]
fn agent_output_deadline_releases_its_subscription() {
    let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
    let registry = AgentRegistry::new();

    assert!(!super::await_agent_output(
        &screen_state,
        &registry,
        "pty-1",
        Duration::ZERO,
    ));
    assert_eq!(screen_state.lock().screen_change_subscriber_count(), 0);
}

#[test]
fn agent_output_cleanup_does_not_close_paste_render_waiter() {
    let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
    let paste_signal = screen_state.lock().arm_screen_change_signal("pty-1");
    let generation = paste_signal.generation();
    let subscription = super::ScreenChangeSubscription::arm(&screen_state, "pty-1");

    drop(subscription);
    screen_state.lock().process("pty-1", b"paste rendered");

    assert!(paste_signal.wait_after(generation, Duration::ZERO));
    assert_eq!(screen_state.lock().screen_change_subscriber_count(), 0);
}

#[test]
fn lifecycle_subscriber_drop_keeps_concurrent_waiter_live() {
    let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
    let first = super::ScreenChangeSubscription::arm(&screen_state, "pty-1");
    let second = super::ScreenChangeSubscription::arm(&screen_state, "pty-1");
    let generation = second.signal.generation();

    drop(first);
    assert_eq!(screen_state.lock().screen_change_subscriber_count(), 1);
    screen_state
        .lock()
        .process("pty-1", b"second waiter remains live");

    assert!(second.signal.wait_after(generation, Duration::ZERO));
    drop(second);
    assert_eq!(screen_state.lock().screen_change_subscriber_count(), 0);
}

#[test]
fn agent_output_teardown_closes_armed_waiter() {
    let pty = Arc::new(TimedMutex::new("pty", PtyManager::new()));
    let (_, session) = pty
        .lock()
        .spawn_with_id(
            Some("pty-1".to_string()),
            None,
            None,
            80,
            24,
            std::collections::HashMap::new(),
        )
        .expect("spawn PTY");
    let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
    assert!(screen_state
        .lock()
        .bind_pty_generation("pty-1", session.generation));
    let registry = Arc::new(AgentRegistry::new());
    let subscription = super::ScreenChangeSubscription::arm(&screen_state, "pty-1");
    let signal = subscription.signal.clone();
    let (armed_tx, armed_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let waiter_screen = screen_state.clone();
    let waiter_registry = registry.clone();
    let waiter = std::thread::spawn(move || {
        let ready = super::await_agent_output_with_signal(
            &waiter_screen,
            &waiter_registry,
            "pty-1",
            &signal,
            Duration::from_secs(1),
            || false,
            || armed_tx.send(()).unwrap(),
        );
        done_tx.send(ready).unwrap();
    });

    armed_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(crate::service::pty_lifecycle::handle_pty_screen_death(
        &pty,
        &screen_state,
        "pty-1",
        session.generation,
    ));

    assert!(!done_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    waiter.join().unwrap();
    drop(subscription);
    assert_eq!(screen_state.lock().screen_change_subscriber_count(), 0);
    pty.lock().kill("pty-1").expect("cleanup PTY");
}

#[test]
fn stale_provider_pty_allows_relaunch_and_cleans_screen_state() {
    let pty = Arc::new(TimedMutex::new("pty", PtyManager::new()));
    let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
    screen_state.lock().process("stale-pty", b"old output");
    let signal = screen_state.lock().arm_screen_change_signal("stale-pty");

    super::terminate_pty_before_relaunch(&pty, &screen_state, "stale-pty")
        .expect("already-removed PTY must allow the configured provider to relaunch");

    assert!(screen_state.lock().get_screen_text("stale-pty").is_none());
    assert_eq!(screen_state.lock().screen_change_subscriber_count(), 0);
    assert!(!signal.wait_after(signal.generation(), Duration::ZERO));
}

#[test]
fn repeated_lifecycle_subscriptions_release_screen_state_entries() {
    let screen_state = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));

    for _ in 0..3 {
        let subscription = super::ScreenChangeSubscription::arm(&screen_state, "pty-1");
        assert_eq!(screen_state.lock().screen_change_subscriber_count(), 1);
        drop(subscription);
        assert_eq!(screen_state.lock().screen_change_subscriber_count(), 0);
    }
}
