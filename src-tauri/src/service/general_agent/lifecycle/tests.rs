use crate::agent::registry::AgentRegistry;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{
    PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};
use crate::pty::manager::PtyManager;
use crate::service::inject_verify::{InjectionError, InjectionResult};
use crate::service::test_fixture::ServiceFixture;
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

/// The observed first-run bug: a bootstrap paste whose submit key was never
/// acknowledged is uncertainty the ledger already holds, not a failed spawn.
#[test]
fn an_unconfirmed_startup_submit_is_not_a_spawn_failure() {
    let unconfirmed = crate::service::delivery_error::DeliveryError::SubmitUnconfirmed {
        tab: "general".into(),
        detail: "delivery acknowledgement not observed".into(),
    };
    assert!(unconfirmed.bytes_written());
    assert_eq!(
        super::launch_delivery_failure("general", Err(unconfirmed)),
        None,
    );
    assert_eq!(super::launch_delivery_failure("general", Ok(())), None);
}

/// The over-correction guard: a refusal that wrote nothing still fails the spawn.
#[test]
fn a_startup_refusal_that_wrote_nothing_still_fails_the_spawn() {
    for error in [
        crate::service::delivery_error::DeliveryError::PtyUnavailable {
            tab: "general".into(),
            detail: "no pty".into(),
        },
        crate::service::delivery_error::DeliveryError::TabBusyRefused {
            tab: "general".into(),
            detail: "booting".into(),
        },
        crate::service::delivery_error::DeliveryError::TabNotReachableOnPeer {
            tab: "general".into(),
        },
    ] {
        let code = error.code();
        let reason = super::launch_delivery_failure("general", Err(error))
            .expect("a pre-write refusal is a spawn failure");
        assert!(reason.starts_with(code), "{reason} must carry {code}");
    }
}

/// A tab at the exact point the first-run launch reaches the binding wait: the
/// payload was written on this PTY under this attempt's nonce, the ledger holds
/// the unconfirmed delivery, and no binding has arrived.
fn tab_after_unconfirmed_launch_delivery() -> crate::multiplexer::types::Tab {
    let mut mux = MultiplexerState::empty();
    super::super::workspace::ensure_general_workspace(&mut mux, true);
    let tab_id = mux
        .resolve_tab_id(super::GENERAL_ALIAS)
        .expect("general Tab");
    let mut pending = PendingDeliveryRecord::new(
        "bootstrap-digest".into(),
        PendingDeliveryChannel::InteractiveInput,
    );
    pending.mark_unconfirmed();
    let tab = mux.get_tab_mut(&tab_id).expect("general Tab");
    tab.pty_id = Some("pty-a".into());
    tab.agent_type = Some("codex".into());
    tab.session_binding_nonce = Some("nonce-a".into());
    tab.reported_session_binding_nonce = None;
    tab.agent_session_id = None;
    tab.boot_stage = Some(BootStage::ProviderReady);
    tab.pending_delivery = Some(pending);
    tab.clone()
}

/// Mirrors `defer_general_agent_start_to_late_binding`: the producer reads the
/// retained delivery, then latches under the shared provisional rule.
fn record_missing_binding(tab: &mut crate::multiplexer::types::Tab) -> (BootStage, String) {
    let delivery = tab.pending_delivery.as_ref().map(|record| record.state());
    let (stage, reason) = super::missing_binding_provisional(delivery);
    let latch = crate::service::boot_stage::provisional_launch_for(tab, "pty-a", &reason)
        .expect("a current launch is latchable while it is unresolved");
    crate::service::boot_stage::apply_boot_stage(tab, stage, Some(&reason));
    tab.provisional_launch = Some(latch);
    (stage, reason)
}

/// The live #477 regression: a first-run launch whose payload reached the PTY
/// and whose binding never arrived was recorded as a terminal spawn failure.
#[test]
fn submit_unconfirmed_does_not_latch_terminal_spawn_failed() {
    let unconfirmed = crate::service::delivery_error::DeliveryError::SubmitUnconfirmed {
        tab: "general".into(),
        detail: "delivery acknowledgement not observed".into(),
    };
    let unconfirmed_code = unconfirmed.code();
    assert!(unconfirmed.bytes_written());
    assert_eq!(
        super::launch_delivery_failure("general", Err(unconfirmed)),
        None,
        "a written payload is never a spawn failure",
    );

    let (stage, reason) =
        super::missing_binding_provisional(Some(PendingDeliveryState::Unconfirmed));

    assert_eq!(stage, BootStage::LaunchBindingUnconfirmed);
    assert_ne!(stage, BootStage::SpawnFailed);
    assert!(stage.is_provisional());
    assert!(stage.carries_reason());
    assert!(
        reason.starts_with(unconfirmed_code),
        "the provisional stage carries its concrete delivery evidence: {reason}",
    );
    assert!(reason.contains(super::BINDING_UNREPORTED), "{reason}");
}

#[test]
fn submit_unconfirmed_stays_provisional_without_binding() {
    let mut tab = tab_after_unconfirmed_launch_delivery();

    let (stage, reason) = record_missing_binding(&mut tab);

    assert_eq!(stage, BootStage::LaunchBindingUnconfirmed);
    assert_eq!(tab.boot_stage, Some(stage));
    assert_ne!(tab.boot_stage, Some(BootStage::SpawnFailed));
    assert!(tab.boot_stage.is_some_and(|stage| stage.is_provisional()));
    assert_eq!(tab.boot_failure_reason.as_deref(), Some(reason.as_str()));
    assert_eq!(
        tab.provisional_launch
            .as_ref()
            .map(|latch| (latch.pty_id.as_str(), latch.binding_nonce.as_str())),
        Some(("pty-a", "nonce-a")),
        "the provisional state is scoped to this PTY and this launch attempt",
    );
    assert_eq!(
        tab.pending_delivery.as_ref().map(|record| record.state()),
        Some(PendingDeliveryState::Unconfirmed),
        "the pending delivery evidence survives the provisional record",
    );
    assert!(
        !tab.launch_session_authenticated(),
        "no binding arrived, so nothing unlocks this launch yet",
    );
}

/// The over-correction guard at the producer: a refusal that wrote nothing is
/// still a spawn failure, and a launch that dies after the provisional record
/// still latches terminally.
#[test]
fn genuine_launch_failure_remains_blocked_after_unconfirmed() {
    for error in [
        crate::service::delivery_error::DeliveryError::PtyUnavailable {
            tab: "general".into(),
            detail: "no pty".into(),
        },
        crate::service::delivery_error::DeliveryError::TabBusyRefused {
            tab: "general".into(),
            detail: "booting".into(),
        },
    ] {
        let code = error.code();
        let reason = super::launch_delivery_failure("general", Err(error))
            .expect("a pre-write refusal is a spawn failure");
        assert!(reason.starts_with(code), "{reason} must carry {code}");
    }

    let mut tab = tab_after_unconfirmed_launch_delivery();
    record_missing_binding(&mut tab);
    crate::service::boot_stage::apply_boot_stage(&mut tab, BootStage::SpawnFailed, Some("PtyEof"));

    assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
    assert_eq!(tab.boot_failure_reason.as_deref(), Some("PtyEof"));
    assert_eq!(tab.provisional_launch, None);
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
fn general_agent_bootstrap_only_before_first_run_completes() {
    let mem = crate::mem::MemManager::open_in_memory().expect("mem store");
    mem.seed_bundled_if_needed().expect("seed bundled mem");
    let seeded_identity = mem
        .get("preference_identity")
        .expect("read seeded identity")
        .expect("seeded identity record");
    assert!(seeded_identity
        .source
        .as_deref()
        .is_some_and(|source| source.starts_with("bundled:")));

    let dir = tempfile::tempdir().expect("temporary directory");
    let bootstrap_path = dir.path().join("bootstrap.md");
    std::fs::write(&bootstrap_path, "BOOTSTRAP_SENTINEL").expect("bootstrap file");

    let startup = super::compose_startup_message(&mem, &bootstrap_path).expect("first run");
    assert!(startup.contains("BOOTSTRAP_SENTINEL"));

    std::fs::write(dir.path().join("identity.md"), "IDENTITY_COMPLETE_SENTINEL")
        .expect("completed identity file");
    assert!(bootstrap_path.exists());
    assert!(super::compose_startup_message(&mem, &bootstrap_path).is_none());
}

#[test]
fn general_agent_launch_uses_system_prompt_channel_when_available() {
    use crate::agent::provider::{AgentLaunchParams, StarterPrompt, SystemPromptDelivery};

    let startup_message = Some("BOOTSTRAP_SENTINEL".to_string());
    let native = super::compose_launch_instructions(
        "GA_INSTRUCTIONS_SENTINEL".into(),
        startup_message.clone(),
        &SystemPromptDelivery::CliFlag("--system".into()),
    );
    let params = AgentLaunchParams {
        task: native.task.clone(),
        starter_prompt: native.starter_prompt,
        ..AgentLaunchParams::default()
    };

    assert!(params
        .task
        .as_deref()
        .is_some_and(|task| task.contains("GA_INSTRUCTIONS_SENTINEL")
            && task.contains("BOOTSTRAP_SENTINEL")));
    assert_eq!(params.starter_prompt, StarterPrompt::AssignedTask);
    assert!(native.startup_message.is_none());
    assert_eq!(native.prompt_delivery, "cli_flag");

    let fallback = super::compose_launch_instructions(
        "GA_INSTRUCTIONS_SENTINEL".into(),
        startup_message,
        &SystemPromptDelivery::External,
    );
    assert!(fallback.task.is_none());
    assert!(fallback
        .startup_message
        .as_deref()
        .is_some_and(|message| message.contains("BOOTSTRAP_SENTINEL")));
    assert_eq!(fallback.starter_prompt, StarterPrompt::NoStarter);
    assert_eq!(fallback.prompt_delivery, "confirmed_fallback");
}

#[test]
fn provider_without_bootstrap_receives_no_interactive_startup_turn() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let bootstrap_path = dir.path().join("bootstrap.md");
    let mem = crate::mem::MemManager::open_in_memory().expect("mem store");
    assert!(super::compose_startup_message(&mem, &bootstrap_path).is_none());
}

#[test]
fn general_agent_launch_uses_capability_backed_permission_bypass() {
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));

    let permission_bypass =
        crate::service::permission_policy::authorize_permission_bypass(&registry, "claude", true)
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
    assert!(
        crate::service::permission_policy::authorize_permission_bypass(
            &registry,
            "missing-provider",
            true,
        )
        .is_none()
    );
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
    let mut process_env = codeterm_util::test_env::EnvGuard::lock();
    process_env.set("CODETERM_API_PORT", "7685");
    process_env.set("CODETERM_API_URL", "http://127.0.0.1:7685");
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
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn general_agent_launch_claim_blocks_initial_shell_exit_teardown() {
    let mut mux = MultiplexerState::empty();
    super::super::workspace::ensure_general_workspace(&mut mux, true);
    let general_id = mux
        .resolve_tab_id(super::GENERAL_ALIAS)
        .expect("general Tab");
    let tab = mux.get_tab_mut(&general_id).expect("general Tab");
    tab.agent_type = Some("codex".into());
    tab.agent_type_locked = true;
    tab.agent_session_id = Some("session-1".into());
    tab.session_binding_nonce = Some("nonce-1".into());
    tab.reported_session_binding_nonce = Some("nonce-1".into());
    tab.boot_stage = Some(BootStage::ProviderReady);
    let fixture = ServiceFixture::new(mux);

    {
        let _claim =
            super::AutomaticLaunchClaim::acquire(&fixture.service.mux, &general_id).unwrap();
        {
            let mut state = fixture.service.mux.write();
            let tab = state.get_tab_mut(&general_id).expect("general Tab");
            assert!(tab.agent_booting);
            assert_eq!(tab.boot_stage, Some(BootStage::PtyStarting));
            tab.boot_stage = Some(BootStage::ProviderReady);
        }
        assert!(!fixture
            .service
            .note_provider_exit_in_surviving_shell(&general_id));
        let state = fixture.service.mux.read();
        let tab = state.get_tab(&general_id).expect("general Tab");

        assert!(tab.agent_booting);
        assert_eq!(tab.boot_stage, Some(BootStage::ProviderReady));
        assert_eq!(tab.agent_type.as_deref(), Some("codex"));
        assert_eq!(tab.agent_session_id.as_deref(), Some("session-1"));
        assert_eq!(tab.session_binding_nonce.as_deref(), Some("nonce-1"));
    }

    {
        let state = fixture.service.mux.read();
        let tab = state.get_tab(&general_id).expect("general Tab");
        assert!(!tab.agent_booting);
        assert_eq!(tab.boot_stage, Some(BootStage::ProviderReady));
    }
    assert!(fixture
        .service
        .note_provider_exit_in_surviving_shell(&general_id));
    let state = fixture.service.mux.read();
    let tab = state.get_tab(&general_id).expect("general Tab");
    assert!(tab.agent_type.is_none());
    assert!(tab.agent_session_id.is_none());
    assert!(tab.session_binding_nonce.is_none());
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
