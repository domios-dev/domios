use super::startup_readiness::reused_provider_tui_owns_launch;
use super::startup_readiness::{
    first_turn_binding_required, input_ready_stage_after_provider_ready,
    launch_waits_for_first_turn_binding,
};
#[path = "tests/structural_wait.rs"]
mod structural_wait;
use super::{
    build_agent_command, command_with_binding_nonce_for_shell, replacement_pty_candidate,
    take_reader_termination, worker_brief_delivery_incident, AgentRegistry, LaunchFailure,
    LaunchGeneration, LaunchRegistry, ProviderSessionBinding, LAUNCH_REGISTRY,
};
use crate::chatbackend::{ChatBackendError, ChatBackendProvider, ChatBackendRegistry};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{
    PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};
use crate::service::boot_stage::{BootStage, BootStageSink};
use crate::service::inject_verify::InjectionResult;
use crate::service::launch_delivery::{LaunchConsumptionOutcome, LaunchDeliveryGate};
use crate::service::orchestrator::prompt_compose::spawn_onboarding_task;
use crate::service::orchestrator::types::SpawnAgentParams;
use crate::service::pty_io::tab_identity_env;
use crate::service::spawn::{SpawnTabSpec, TabCommand};
use codeterm_util::lock_hold::TimedRwLock;
use parking_lot::Mutex;
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::Barrier;

#[derive(Default)]
struct DeliverySink(Mutex<Vec<&'static str>>);

impl BootStageSink for DeliverySink {
    fn record(&self, _: &str, stage: crate::service::boot_stage::BootStage, _: Option<&str>) {
        self.0.lock().push(stage.as_str());
    }
}

fn delivery_fixture() -> (
    Arc<TimedRwLock<MultiplexerState>>,
    String,
    LaunchDeliveryGate,
) {
    let (mut state, tab_id) = MultiplexerState::new();
    state.get_tab_mut(&tab_id).unwrap().pending_delivery = Some(PendingDeliveryRecord::new(
        "worker-brief".into(),
        PendingDeliveryChannel::InteractiveInput,
    ));
    let mux = Arc::new(TimedRwLock::new("worker-brief-test", state));
    let mut gate = LaunchDeliveryGate::new();
    gate.mark_shell_ready();
    gate.mark_provider_ready();
    (mux, tab_id, gate)
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn resume_binding_on_first_turn_accepts_input_at_provider_ready() {
    use crate::agent::test_provider::MockProvider;

    let (mut mux, tab_id) = MultiplexerState::new();
    let pty_id = "resume-first-turn-pty";
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("resume-first-turn-fixture".into());
        tab.agent_session_id = Some("saved-session".into());
        tab.session_binding_nonce = Some("resume-first-turn-nonce".into());
        tab.pty_id = Some(pty_id.into());
        tab.restored_from_snapshot = true;
    }
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);
    harness.service.agent_registry.register_provider(Box::new(
        MockProvider::new("resume-first-turn-fixture")
            .with_session_starts_on_first_user_turn()
            .with_provider_readiness_strategy(
                crate::agent::provider::ProviderReadinessStrategy::StructuralComposer {
                    timeout: std::time::Duration::from_secs(30),
                },
            ),
    ));
    assert!(harness
        .service
        .agent_registry
        .with_provider("resume-first-turn-fixture", |provider| {
            provider.provider_readiness_strategy().requires_composer()
        })
        .unwrap());

    assert!(launch_waits_for_first_turn_binding(
        &harness.service,
        &tab_id,
        "resume-first-turn-fixture",
        false,
        crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
    ));
    assert!(launch_waits_for_first_turn_binding(
        &harness.service,
        &tab_id,
        "resume-first-turn-fixture",
        true,
        crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
    ));
    assert!(!launch_waits_for_first_turn_binding(
        &harness.service,
        &tab_id,
        "resume-first-turn-fixture",
        true,
        crate::service::launch_diagnostics::AgentLaunchOrigin::VisibleSpawn,
    ));
    harness
        .service
        .agent_registry
        .register_provider(Box::new(MockProvider::new("claude-like-resume-fixture")));
    assert!(!launch_waits_for_first_turn_binding(
        &harness.service,
        &tab_id,
        "claude-like-resume-fixture",
        true,
        crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
    ));

    let mut launch = LaunchGeneration::begin(&tab_id, pty_id.into());
    let input_ready_stage = input_ready_stage_after_provider_ready(true);
    assert_eq!(input_ready_stage, BootStage::AgentReady);
    assert!(!launch.await_provider_ack(std::time::Duration::ZERO));
    assert!(include_str!("owner_prompt_wait.rs").contains("await_lazy_session_input_readiness"));
    let launch_source = include_str!("../launch.rs");
    let provider_up = launch_source
        .find("owner_prompt_wait::await_input_readiness")
        .expect("first-turn readiness waits for provider composer evidence");
    let input_ready = launch_source
        .find("emit_first_turn_input_ready")
        .expect("first-turn readiness emits the input-accepting stage");
    assert!(provider_up < input_ready);
    assert!(launch.emit_first_turn_input_ready(&harness.service));
    let mux = harness.service.mux.read();
    let tab = mux.get_tab(&tab_id).expect("restored tab remains present");
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert_eq!(tab.boot_failure_reason, None);
    assert!(!tab.launch_session_authenticated());
    drop(mux);
    LAUNCH_REGISTRY.lock().finish(&launch);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn has_startup_task_turn_reflects_the_actual_launch_command() {
    use crate::agent::provider::{
        AgentProvider, StarterPrompt, SystemPromptDelivery, TaskDelivery,
    };
    use crate::agent::test_provider::MockProvider;

    let (mux, _) = MultiplexerState::new();
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);
    let codex = MockProvider::with_system_prompt_flag("codex-shaped", "-c developer_instructions")
        .with_task_delivery(TaskDelivery::InlineFlag)
        .with_hooks()
        .with_session_starts_on_first_user_turn();
    assert!(matches!(codex.task_delivery(), TaskDelivery::InlineFlag));
    assert!(codex.has_hooks());
    harness
        .service
        .agent_registry
        .register_provider(Box::new(codex));
    let opencode = MockProvider::new("opencode-shaped")
        .with_task_delivery(TaskDelivery::InlineFlag)
        .with_session_config_delivery()
        .with_hooks()
        .with_session_starts_on_first_user_turn();
    assert_eq!(
        opencode.system_prompt_delivery(),
        SystemPromptDelivery::SessionConfig
    );
    harness
        .service
        .agent_registry
        .register_provider(Box::new(opencode));

    for provider_id in ["codex-shaped", "opencode-shaped"] {
        let plan = crate::service::startup_turn::resolve(
            &harness.service.agent_registry,
            provider_id,
            StarterPrompt::NoStarter,
            None,
            Some("opaque-team-bootstrap-system-prompt-9ef1"),
            true,
            None,
        );
        assert!(plan.expected_turn.is_none());
        assert!(!plan.first_turn_will_be_delivered);
        assert!(first_turn_binding_required(
            true,
            plan.first_turn_will_be_delivered,
            false
        ));
    }

    harness.service.agent_registry.register_provider(Box::new(
        MockProvider::new("hookless-inline-starter")
            .with_task_delivery(TaskDelivery::InlineFlag)
            .with_starter_prompt_text("opaque-baked-starter-c011")
            .with_session_starts_on_first_user_turn(),
    ));
    let baked = crate::service::startup_turn::resolve(
        &harness.service.agent_registry,
        "hookless-inline-starter",
        StarterPrompt::AssignedTask,
        None,
        Some("opaque-inline-task-80bd"),
        true,
        None,
    );
    assert!(baked.expected_turn.is_none());
    assert!(baked.first_turn_will_be_delivered);
    assert!(!first_turn_binding_required(
        true,
        baked.first_turn_will_be_delivered,
        false,
    ));
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn taskless_lazy_session_spawn_becomes_input_ready_from_provider_evidence() {
    use crate::agent::test_provider::MockProvider;

    let (mut mux, tab_id) = MultiplexerState::new();
    let pty_id = "taskless-lazy-session-pty";
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("lazy-session-fixture".into());
        tab.session_binding_nonce = Some("taskless-lazy-session-nonce".into());
        tab.pty_id = Some(pty_id.into());
    }
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);
    harness.service.agent_registry.register_provider(Box::new(
        MockProvider::new("lazy-session-fixture")
            .with_session_config_delivery()
            .with_session_starts_on_first_user_turn()
            .with_provider_readiness_strategy(
                crate::agent::provider::ProviderReadinessStrategy::StructuralComposer {
                    timeout: std::time::Duration::from_secs(30),
                },
            ),
    ));

    assert!(harness
        .service
        .agent_registry
        .with_provider("lazy-session-fixture", |provider| {
            provider.provider_readiness_strategy().requires_composer()
        })
        .unwrap());

    let no_task_turn = crate::service::startup_turn::resolve(
        &harness.service.agent_registry,
        "lazy-session-fixture",
        crate::agent::provider::StarterPrompt::NoStarter,
        None,
        None,
        true,
        None,
    );
    assert!(launch_waits_for_first_turn_binding(
        &harness.service,
        &tab_id,
        "lazy-session-fixture",
        no_task_turn.first_turn_will_be_delivered,
        crate::service::launch_diagnostics::AgentLaunchOrigin::VisibleSpawn,
    ));
    let assigned_task_turn = crate::service::startup_turn::resolve(
        &harness.service.agent_registry,
        "lazy-session-fixture",
        crate::agent::provider::StarterPrompt::NoStarter,
        Some(&crate::service::launch_delivery::FallbackDeliverySpec {
            channel: PendingDeliveryChannel::InteractiveInput,
            payload: "opaque-resolved-first-turn-4209".into(),
        }),
        None,
        true,
        None,
    );
    assert!(!launch_waits_for_first_turn_binding(
        &harness.service,
        &tab_id,
        "lazy-session-fixture",
        assigned_task_turn.first_turn_will_be_delivered,
        crate::service::launch_diagnostics::AgentLaunchOrigin::VisibleSpawn,
    ));
    assert!(!launch_waits_for_first_turn_binding(
        &harness.service,
        &tab_id,
        "lazy-session-fixture",
        true,
        crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
    ));
    let mut launch = LaunchGeneration::begin(&tab_id, pty_id.into());
    assert_eq!(
        input_ready_stage_after_provider_ready(true),
        BootStage::AgentReady,
    );
    assert!(!launch.await_provider_ack(std::time::Duration::ZERO));

    assert!(include_str!("owner_prompt_wait.rs").contains("await_lazy_session_input_readiness"));
    let launch_source = include_str!("../launch.rs");
    let provider_evidence = launch_source
        .find("owner_prompt_wait::await_input_readiness")
        .expect("lazy launch waits for provider-owned composer evidence");
    let input_ready = launch_source
        .find("emit_first_turn_input_ready")
        .expect("lazy launch accepts input before its binding turn");
    assert!(provider_evidence < input_ready);
    assert!(launch.emit_first_turn_input_ready(&harness.service));

    let mux = harness.service.mux.read();
    let tab = mux.get_tab(&tab_id).expect("taskless tab remains present");
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert_eq!(tab.boot_failure_reason, None);
    assert!(tab.agent_session_id.is_none());
    assert!(tab.pending_delivery.is_none());
    drop(mux);
    LAUNCH_REGISTRY.lock().finish(&launch);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn owner_prompt_cleared_on_lazy_session_launch_moves_to_agent_ready_without_first_turn() {
    use crate::agent::provider::ProviderReadinessStrategy;
    use crate::agent::test_provider::MockProvider;
    use crate::service::launch_readiness::{
        arm, observe, owner_prompt_cleared, owner_prompt_pending, register_input_wake,
        LaunchAttempt, LaunchObservation,
    };

    let (mut mux, tab_id) = MultiplexerState::new();
    let pty_id = "lazy-owner-prompt-stage-pty";
    let binding_nonce = "lazy-owner-prompt-stage-nonce";
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("lazy-owner-prompt-stage".into());
        tab.session_binding_nonce = Some(binding_nonce.into());
        tab.pty_id = Some(pty_id.into());
    }
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);
    harness.service.agent_registry.register_provider(Box::new(
        MockProvider::new("lazy-owner-prompt-stage")
            .with_session_starts_on_first_user_turn()
            .with_provider_readiness_strategy(ProviderReadinessStrategy::StructuralComposer {
                timeout: std::time::Duration::from_secs(30),
            }),
    ));
    let attempt = LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: pty_id.into(),
        binding_nonce: binding_nonce.into(),
    };
    arm(attempt.clone());
    owner_prompt_pending(&tab_id, "opaque-owner-trust-question");
    let signal = harness
        .service
        .screen_state
        .lock()
        .arm_screen_change_signal(pty_id);
    let _wake = register_input_wake(&attempt, signal.clone());
    let generation = signal.generation();
    let waiter_signal = signal.clone();
    let resumed = std::thread::spawn(move || {
        waiter_signal.wait_after(generation, std::time::Duration::from_secs(5))
    });

    owner_prompt_cleared(&tab_id);

    assert!(
        resumed.join().unwrap(),
        "prompt clear resumes lazy readiness"
    );
    assert!(harness
        .service
        .agent_registry
        .with_provider("lazy-owner-prompt-stage", |provider| {
            provider.provider_readiness_strategy().requires_composer()
        })
        .unwrap());
    drop(_wake);
    assert_eq!(
        observe(&attempt, std::time::Instant::now()),
        LaunchObservation::Deadline
    );

    let mut launch = LaunchGeneration::begin(&tab_id, pty_id.into());
    assert!(launch.emit_first_turn_input_ready(&harness.service));
    let mux = harness.service.mux.read();
    let tab = mux
        .get_tab(&tab_id)
        .expect("lazy launch tab remains present");
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert!(tab.agent_session_id.is_none());
    drop(mux);
    LAUNCH_REGISTRY.lock().finish(&launch);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn team_pair_codex_orchestrator_reaches_agent_ready_without_starter_turn() {
    use crate::agent::provider::StarterPrompt;
    use crate::agent::test_provider::MockProvider;

    let (mut mux, tab_id) = MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().agent_type = Some("pair-lazy-session".into());
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);
    harness.service.agent_registry.register_provider(Box::new(
        MockProvider::with_system_prompt_flag("pair-lazy-session", "-c developer_instructions")
            .with_task_delivery(crate::agent::provider::TaskDelivery::InlineFlag)
            .with_hooks()
            .with_session_starts_on_first_user_turn(),
    ));
    let startup_turn = crate::service::startup_turn::resolve(
        &harness.service.agent_registry,
        "pair-lazy-session",
        StarterPrompt::NoStarter,
        None,
        Some("opaque-team-bootstrap-instructions-f7a1"),
        true,
        None,
    );
    assert!(startup_turn.expected_turn.is_none());
    assert!(!startup_turn.first_turn_will_be_delivered);
    assert!(launch_waits_for_first_turn_binding(
        &harness.service,
        &tab_id,
        "pair-lazy-session",
        startup_turn.first_turn_will_be_delivered,
        crate::service::launch_diagnostics::AgentLaunchOrigin::OrchestratedSpawn,
    ));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn codex_orchestrator_waits_for_owner_trust_then_lazy_readiness() {
    use crate::agent::provider::{StarterPrompt, TaskDelivery};
    use crate::agent::test_provider::MockProvider;

    let (mut mux, tab_id) = MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().agent_type = Some("trust-lazy-session".into());
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);
    harness.service.agent_registry.register_provider(Box::new(
        MockProvider::with_system_prompt_flag("trust-lazy-session", "-c developer_instructions")
            .with_task_delivery(TaskDelivery::InlineFlag)
            .with_hooks()
            .with_session_starts_on_first_user_turn(),
    ));
    let turn_plan = crate::service::startup_turn::resolve(
        &harness.service.agent_registry,
        "trust-lazy-session",
        StarterPrompt::NoStarter,
        None,
        Some("opaque-untrusted-team-bootstrap-bc10"),
        true,
        None,
    );
    assert!(!turn_plan.first_turn_will_be_delivered);

    crate::service::launch_readiness::owner_prompt_pending(&tab_id, "trust-question-a813");
    assert!(crate::service::launch_readiness::owner_prompt_is_active(
        &tab_id
    ));
    let service = harness.service.clone();
    let observer_tab_id = tab_id.clone();
    let observer = std::thread::spawn(move || {
        assert_eq!(
            crate::service::launch_readiness::await_owner_prompt_resolution(&observer_tab_id),
            None
        );
        launch_waits_for_first_turn_binding(
            &service,
            &observer_tab_id,
            "trust-lazy-session",
            turn_plan.first_turn_will_be_delivered,
            crate::service::launch_diagnostics::AgentLaunchOrigin::OrchestratedSpawn,
        )
    });
    crate::service::launch_readiness::owner_prompt_cleared(&tab_id);

    assert!(observer.join().unwrap());
}

#[test]
fn restored_lazy_session_first_turn_is_delivered_once_and_binds() {
    let mut driver = LaunchRegistry::default();
    let launch = driver.begin("lazy-first-turn", "lazy-first-turn-pty".into());
    bind(&mut driver, &launch, "lazy-first-turn-nonce");
    let payload_writes = Cell::new(0);
    let deliver_first_turn = || {
        payload_writes.set(payload_writes.get() + 1);
        driver.record_provider_session(
            "lazy-first-turn",
            &ProviderSessionBinding {
                pty_id: "lazy-first-turn-pty".into(),
                binding_nonce: "lazy-first-turn-nonce".into(),
            },
        )
    };

    assert!(first_turn_binding_required(true, true, true));
    assert!(!first_turn_binding_required(false, true, true));
    assert!(deliver_first_turn());
    assert_eq!(payload_writes.get(), 1);
    assert!(launch.await_provider_ack(std::time::Duration::ZERO));
}

#[test]
fn post_write_uncertainty_keeps_one_payload_pending_without_failing_launch() {
    let (mux, tab_id, gate) = delivery_fixture();
    let sink = DeliverySink::default();
    let payload_writes = Cell::new(0);
    let consumption = gate.consume_injection(&mux, &tab_id, &sink, || {
        payload_writes.set(payload_writes.get() + 1);
        Ok(InjectionResult {
            bytes_written: "worker brief".len(),
            echo_observed: false,
            submit_observed: false,
            submit_anomaly: false,
            submit_key_writes: 0,
            elapsed: std::time::Duration::ZERO,
        })
    });
    let cleanup_calls = Cell::new(0);
    if consumption
        .as_ref()
        .is_ok_and(|receipt| receipt.outcome == LaunchConsumptionOutcome::Confirmed)
    {
        cleanup_calls.set(cleanup_calls.get() + 1);
    }
    let pending = mux
        .read()
        .get_tab(&tab_id)
        .unwrap()
        .pending_delivery
        .clone()
        .unwrap();

    assert_eq!(payload_writes.get(), 1);
    assert_eq!(cleanup_calls.get(), 0);
    assert_eq!(pending.state(), PendingDeliveryState::Unconfirmed);
    let error = consumption.unwrap_err();
    assert_eq!(error.delivery.code(), "submit_unconfirmed");
    assert!(!error.surfaces_as_launch_failure());
}

#[test]
fn pre_write_refusal_remains_a_launch_failure() {
    let (mux, tab_id, gate) = delivery_fixture();
    let sink = DeliverySink::default();
    let transaction_calls = Cell::new(0);
    let consumption = gate.consume_injection(&mux, &tab_id, &sink, || {
        transaction_calls.set(transaction_calls.get() + 1);
        Err(
            crate::service::inject_verify::InjectionError::AgentInputUnavailable {
                pty_id: "closed-before-write".into(),
            },
        )
    });

    let error = consumption.unwrap_err();
    assert_eq!(transaction_calls.get(), 1);
    assert_eq!(error.delivery.code(), "tab_busy_refused");
    assert!(!error.delivery.bytes_written());
    assert!(error.surfaces_as_launch_failure());
}

#[test]
fn worker_brief_is_one_payload_and_one_turn() {
    let (mux, tab_id, gate) = delivery_fixture();
    let payload = "first line\nsecond line\nthird line";
    let payloads = RefCell::new(Vec::new());
    let turns = Cell::new(0);
    let queued_characters = Cell::new(0);

    let consumption = gate
        .consume_injection(&mux, &tab_id, &DeliverySink::default(), || {
            crate::service::pty_io::submit_transaction::submit_entry(
                crate::service::pty_io::submit_transaction::PtySubmitEntry::Direct,
                payload,
                |submitted| {
                    payloads.borrow_mut().push(submitted.to_string());
                    turns.set(turns.get() + 1);
                    Ok(InjectionResult {
                        bytes_written: submitted.len(),
                        echo_observed: true,
                        submit_observed: true,
                        submit_anomaly: false,
                        submit_key_writes: 1,
                        elapsed: std::time::Duration::ZERO,
                    })
                },
            )
        })
        .unwrap();

    assert_eq!(consumption.outcome, LaunchConsumptionOutcome::Confirmed);
    assert!(consumption.attempt_id.is_some());
    assert_eq!(payloads.borrow().as_slice(), &[payload]);
    assert_eq!(turns.get(), 1);
    assert_eq!(queued_characters.get(), 0);
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .pending_delivery
            .as_ref()
            .unwrap()
            .state(),
        PendingDeliveryState::AcceptedWeak
    );
    let launch_source = include_str!("../launch.rs");
    assert!(launch_source.contains("submit_text_with_key"));
    assert!(!launch_source.contains("send_text_detached"));
}

#[test]
fn worker_brief_failure_alert_states_impact_cleanup_and_fallback() {
    let (title, body) =
        worker_brief_delivery_incident("worker-tab", "submit_unconfirmed: composer retained");

    assert_eq!(title, "Orchestration incident: launch brief undelivered");
    assert!(body.contains("Impact: Tab worker-tab was not marked ready"));
    assert!(body.contains("Cleanup: confirmed-delivery cleanup was skipped"));
    assert!(body.contains("Fallback:"));
    assert!(body.contains("pending brief was retained"));
    assert!(body.contains("submit_unconfirmed"));
}

fn orchestrator_params(task: Option<&str>) -> SpawnAgentParams {
    SpawnAgentParams {
        agent: Some("test-provider".into()),
        task: task.map(str::to_string),
        role: Some("Orchestrator".into()),
        ..Default::default()
    }
}

#[test]
fn replacement_pty_must_be_new_and_live() {
    assert_eq!(
        replacement_pty_candidate(Some("new"), "closed", true).as_deref(),
        Some("new")
    );
    assert!(replacement_pty_candidate(Some("closed"), "closed", true).is_none());
    assert!(replacement_pty_candidate(Some("new"), "closed", false).is_none());
    assert!(replacement_pty_candidate(None, "closed", true).is_none());
}

#[test]
fn restore_launch_requires_bare_shell_evidence_before_writing_commands() {
    let source = include_str!("../launch.rs");
    assert!(!source.contains("pty_io::await_shell_prompt("));
    assert_eq!(
        source.matches("pty_io::await_bare_shell_prompt(").count(),
        2
    );
}

#[test]
fn powershell_launch_inherits_an_escaped_binding_nonce() {
    let command = command_with_binding_nonce_for_shell(
        "Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue; claude --resume",
        "nonce'a",
        "pty-a",
        true,
        None,
    );
    assert_eq!(
        command,
        "$env:CODETERM_SESSION_BINDING_NONCE='nonce''a'; $env:CODETERM_STARTUP_ATTEMPT_ID='pty-a'; Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue; claude --resume"
    );
}

#[test]
fn posix_launch_inherits_an_escaped_binding_nonce() {
    let command = command_with_binding_nonce_for_shell(
        "unset CLAUDECODE && claude --resume",
        "nonce'a",
        "pty-a",
        false,
        None,
    );
    assert_eq!(
        command,
        "export CODETERM_SESSION_BINDING_NONCE='nonce'\"'\"'a'; export CODETERM_STARTUP_ATTEMPT_ID='pty-a'; unset CLAUDECODE && claude --resume"
    );
}

#[test]
fn production_driver_rejects_a_stale_generation_before_emit_or_write() {
    let mut driver = LaunchRegistry::default();
    let stale = driver.begin("binding-stale", "binding-stale-old".into());
    let current = driver.begin("binding-stale", "binding-stale-new".into());
    assert!(!driver.owns(&stale));
    assert!(driver.owns(&current));
    assert!(!driver.finish(&stale));
    assert!(driver.owns(&current));
}

#[test]
fn production_driver_write_claim_rejects_a_superseded_generation() {
    let stale = LaunchGeneration::begin("binding-write", "binding-write-old".into());
    let _current = LaunchGeneration::begin("binding-write", "binding-write-new".into());
    assert_eq!(stale.write_if_current(|| true), None);
}

#[test]
fn barrier_commit_cannot_be_overwritten_by_concurrent_begin() {
    let launch = LaunchGeneration::begin("binding-race-emit", "binding-emit-old".into());
    let barrier = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let held = barrier.clone();
    let unhold = release.clone();
    let worker = std::thread::spawn(move || {
        launch.commit_if_current(|| {
            held.wait();
            unhold.wait();
        })
    });
    barrier.wait();
    release.wait();
    let replacement = LaunchGeneration::begin("binding-race-emit", "binding-emit-new".into());
    assert!(worker.join().unwrap());
    assert_eq!(replacement.write_if_current(|| ()), Some(()));
}

#[test]
fn barrier_reader_eof_for_old_pty_cannot_take_replacement() {
    let old = LaunchGeneration::begin("binding-race-eof", "binding-eof-old".into());
    let replacement = LaunchGeneration::begin("binding-race-eof", "binding-eof-new".into());
    assert!(!take_reader_termination("binding-eof-old", |_| {}));
    assert_eq!(replacement.write_if_current(|| ()), Some(()));
    assert_eq!(old.write_if_current(|| ()), None);
}

#[test]
fn production_driver_reports_reused_reader_eof_for_its_current_pty() {
    let mut driver = LaunchRegistry::default();
    let launch = driver.begin("binding-reader", "binding-reader-live".into());
    let terminated = driver
        .take_for_pty("binding-reader-live")
        .expect("registered reader");
    assert_eq!(terminated.tab_id, "binding-reader");
    assert_eq!(terminated.generation, launch.generation);
    assert!(!driver.owns(&launch));
    assert_eq!(LaunchFailure::PtyEof.reason(), "PtyEof");
}

#[test]
fn reused_success_finishes_before_a_later_reader_eof() {
    let mut driver = LaunchRegistry::default();
    let reused = driver.begin("binding-reused", "binding-reused-live".into());
    assert!(driver.finish(&reused));
    assert!(driver.take_for_pty("binding-reused-live").is_none());
}

#[test]
fn production_driver_replaces_pty_and_reader_generation_together() {
    let mut driver = LaunchRegistry::default();
    let original = driver.begin("binding-replace", "binding-replace-old".into());
    let replacement = driver.begin("binding-replace", "binding-replace-new".into());
    assert!(!driver.owns(&original));
    assert!(driver.owns(&replacement));
    assert!(driver.take_for_pty("binding-replace-old").is_none());
    assert_eq!(
        driver
            .take_for_pty("binding-replace-new")
            .unwrap()
            .generation,
        replacement.generation
    );
}

#[test]
fn production_driver_uses_typed_terminal_causes() {
    assert_eq!(
        LaunchFailure::ReaderSetup("closed".into()).reason(),
        "ReaderSetupFailed: closed"
    );
    assert_eq!(LaunchFailure::PtyEof.reason(), "PtyEof");
    assert_eq!(
        LaunchFailure::ShellReadyTimeout.reason(),
        "ShellReadyTimeout"
    );
}

#[test]
fn plain_orchestrator_no_task_attaches_nothing() {
    let spec = SpawnTabSpec::from_agent_params(orchestrator_params(None));
    assert_eq!(spec.parent_tab_id, None);
    match spec.command {
        TabCommand::Agent(agent) => {
            assert_eq!(agent.role.as_deref(), Some("Orchestrator"));
            assert_eq!(agent.task, None);
            assert_eq!(agent.worker_id, None);
        }
        other => panic!("expected agent spawn state, got {other:?}"),
    }
}

#[test]
fn plain_orchestrator_role_metadata_uses_the_non_task_channel() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    mux.set_orchestrator_role(&tab_id).unwrap();
    let tab = mux.get_tab(&tab_id).unwrap();
    // The identity channel carries structural authority, not the display label.
    assert_eq!(
        tab_identity_env(tab).get("CODETERM_ROLE"),
        Some(&"Manager".to_string())
    );
}

#[test]
fn identity_env_exports_team_id_when_bound_and_omits_it_when_absent() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&workspace_id).unwrap();

    // Absent Team → the key is omitted, never blank: launched shells inherit no CODETERM_TEAM_ID.
    let tab = mux.get_tab(&tab_id).unwrap();
    assert!(!tab_identity_env(tab).contains_key("CODETERM_TEAM_ID"));

    // Bound Team → exported verbatim so the launched shell groups under its Team.
    mux.get_tab_mut(&tab_id).unwrap().team_id = Some("team_c4bc03af".into());
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(
        tab_identity_env(tab).get("CODETERM_TEAM_ID"),
        Some(&"team_c4bc03af".to_string())
    );
}

fn bind(driver: &mut LaunchRegistry, launch: &LaunchGeneration, nonce: &str) {
    assert!(driver.bind_provider_session(launch, nonce.into()));
}

/// A launch failure must not be outranked by a binding proof when the failure
/// is itself the evidence that the proving process is gone.
#[test]
fn process_death_faults_void_the_binding_proof_that_deadlines_do_not() {
    for failure in [
        LaunchFailure::PtySpawn("no pty".into()),
        LaunchFailure::PtyEof,
        LaunchFailure::TabClosed,
        LaunchFailure::ReaderSetup("no reader".into()),
        LaunchFailure::ReaderRuntime("reader died".into()),
    ] {
        assert!(
            failure.proves_process_gone(),
            "{failure:?} is evidence the process is gone",
        );
    }
    for failure in [
        LaunchFailure::ReaderReadyTimeout,
        LaunchFailure::ShellReadyTimeout,
        LaunchFailure::LaunchWrite("write refused".into()),
        LaunchFailure::TaskDelivery("submit_unconfirmed".into()),
        LaunchFailure::SessionBindingNotObserved("deadline elapsed".into()),
    ] {
        assert!(
            !failure.proves_process_gone(),
            "{failure:?} is an observer giving up, not a dead process",
        );
    }
}

#[test]
fn shell_idle_to_working_cannot_resolve_launch_readiness() {
    let launch = LaunchRegistry::default().begin("shell-transition", "pty-a".into());
    assert!(!launch.await_provider_ack(std::time::Duration::ZERO));
}

#[test]
fn shell_working_to_idle_cannot_resolve_launch_readiness() {
    let launch = LaunchRegistry::default().begin("shell-transition", "pty-a".into());
    assert!(!launch.await_provider_ack(std::time::Duration::ZERO));
}

#[test]
fn matching_authenticated_session_binding_resolves_readiness() {
    let mut driver = LaunchRegistry::default();
    let launch = driver.begin("matching", "pty-a".into());
    bind(&mut driver, &launch, "nonce-a");
    assert!(driver.record_provider_session(
        "matching",
        &ProviderSessionBinding {
            pty_id: "pty-a".into(),
            binding_nonce: "nonce-a".into()
        }
    ));
    assert!(launch.await_provider_ack(std::time::Duration::ZERO));
}

#[test]
fn old_generation_on_the_same_pty_is_ignored() {
    let mut driver = LaunchRegistry::default();
    let old = driver.begin("same-pty", "pty-a".into());
    bind(&mut driver, &old, "old-nonce");
    let current = driver.begin("same-pty", "pty-a".into());
    bind(&mut driver, &current, "current-nonce");
    assert!(!driver.record_provider_session(
        "same-pty",
        &ProviderSessionBinding {
            pty_id: "pty-a".into(),
            binding_nonce: "old-nonce".into()
        }
    ));
    assert!(!current.await_provider_ack(std::time::Duration::ZERO));
}

#[test]
fn provider_binding_for_a_wrong_pty_is_ignored() {
    let mut driver = LaunchRegistry::default();
    let launch = driver.begin("wrong-pty", "pty-a".into());
    bind(&mut driver, &launch, "nonce-a");
    assert!(!driver.record_provider_session(
        "wrong-pty",
        &ProviderSessionBinding {
            pty_id: "pty-b".into(),
            binding_nonce: "nonce-a".into()
        }
    ));
}

#[test]
fn provider_binding_with_a_wrong_nonce_is_ignored() {
    let mut driver = LaunchRegistry::default();
    let launch = driver.begin("wrong-nonce", "pty-a".into());
    bind(&mut driver, &launch, "nonce-a");
    assert!(!driver.record_provider_session(
        "wrong-nonce",
        &ProviderSessionBinding {
            pty_id: "pty-a".into(),
            binding_nonce: "nonce-b".into()
        }
    ));
}

#[test]
fn launch_readiness_requires_binding_evidence_not_screen_heuristics() {
    use crate::service::launch_readiness::LaunchSettlement;

    assert!(LaunchSettlement::Ready.authorizes_agent_ready(true));
    assert!(!LaunchSettlement::Ready.authorizes_agent_ready(false));
    assert!(!LaunchSettlement::AwaitingFirstTurn.authorizes_agent_ready(true));
    let provisional = LaunchSettlement::Provisional {
        stage: BootStage::LaunchDeadlineElapsed,
        reason: "binding was not observed".into(),
    };
    assert!(!provisional.authorizes_agent_ready(false));
    assert!(provisional.authorizes_agent_ready(true));
    assert!(!reused_provider_tui_owns_launch(
        true,
        false,
        Some("codex"),
        "codex",
        true,
    ));
    assert!(reused_provider_tui_owns_launch(
        true,
        true,
        Some("codex"),
        "codex",
        true,
    ));
    assert_eq!(
        LaunchFailure::SessionBindingNotObserved("deadline elapsed".into()).reason(),
        "SessionBindingNotObserved: deadline elapsed"
    );
}

#[test]
fn system_capable_pty_launch_carries_manager_bootstrap_without_fallback_content() {
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    let content = "## CodeTerm manager identity\nTeam Alpha";
    let command = build_agent_command(
        "claude",
        &orchestrator_params(None),
        Some(content),
        None,
        &registry,
    );
    assert!(command.contains("--append-system-prompt-file"));
    assert!(!command.contains(content));
}

/// A composed launch prompt the test itself supplies, so every occurrence
/// counted below came from the resume path rather than a shipped constant.
const RESUME_COMPOSED_PROMPT: &str = "## CodeTerm resume identity\nsentinel-4f2a";

fn resume_params(provider: &str) -> SpawnAgentParams {
    SpawnAgentParams {
        agent: Some(provider.into()),
        resume_session_id: Some("session-77".into()),
        model: Some("fixture-model".into()),
        role: Some("Worker".into()),
        ..Default::default()
    }
}

/// Claude's `--resume` drops the launch-time system prompt, so it declares
/// re-application. The prompt travels in the file the declared flag names —
/// counting occurrences in the file is what proves it arrives exactly once.
#[test]
fn reapply_class_resume_carries_the_composed_prompt_exactly_once() {
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    let command = build_agent_command(
        "claude",
        &resume_params("claude"),
        Some(RESUME_COMPOSED_PROMPT),
        None,
        &registry,
    );

    assert_eq!(command.matches("--resume").count(), 1);
    assert_eq!(command.matches("--append-system-prompt-file").count(), 1);
    let path = command
        .rsplit_once("--append-system-prompt-file ")
        .expect("declared flag carries a path")
        .1
        .trim()
        .trim_matches(['\'', '"']);
    let delivered = std::fs::read_to_string(path).expect("prompt file the resume command names");
    assert_eq!(delivered.matches(RESUME_COMPOSED_PROMPT).count(), 1);
}

/// The discriminator: same composed content, same reachable resume branch,
/// but the provider declares nothing — so re-application must not happen.
/// A CliFlag descriptor means the host DOES hold system-prompt content here,
/// which is what makes the zero meaningful rather than vacuous.
#[test]
fn retain_class_resume_carries_the_composed_prompt_zero_times() {
    use crate::agent::test_provider::MockProvider;

    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(MockProvider::with_system_prompt_flag(
        "retaining",
        "--system",
    )));
    let command = build_agent_command(
        "retaining",
        &resume_params("retaining"),
        Some(RESUME_COMPOSED_PROMPT),
        None,
        &registry,
    );

    assert!(command.contains("--resume session-77"));
    assert_eq!(command.matches(RESUME_COMPOSED_PROMPT).count(), 0);
    assert_eq!(command.matches("--system").count(), 0);
    assert!(command.contains("--model fixture-model"));
}

/// An undeclared provider keeps today's behaviour exactly: a bare resume plus
/// the launch args, and no prompt content of any kind.
#[test]
fn undeclared_provider_resume_keeps_todays_behaviour() {
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "undeclared",
    )));
    let command = build_agent_command(
        "undeclared",
        &resume_params("undeclared"),
        Some(RESUME_COMPOSED_PROMPT),
        None,
        &registry,
    );

    assert_eq!(command.matches(RESUME_COMPOSED_PROMPT).count(), 0);
    assert_eq!(
        command, "undeclared-cli --resume session-77 --model fixture-model",
        "undeclared resume is the base command plus launch args, nothing else"
    );
}

#[test]
fn free_form_unknown_model_is_preserved_in_launch_command() {
    let provider_id = "free-form-launch-command-test";
    let requested_model = "opencode/qa-free-form-id";
    crate::agent::model_catalog::register(
        provider_id,
        crate::agent::model_catalog::RegisteredDefaults {
            models: vec![crate::agent::provider::ModelInfo {
                default_reasoning_effort: None,
                id: "opencode/big-pickle".into(),
                display_name: "Big Pickle".into(),
                ..Default::default()
            }],
            default: Some("opencode/big-pickle".into()),
        },
    );
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        provider_id,
    )));
    let params = SpawnAgentParams {
        agent: Some(provider_id.into()),
        model: Some(requested_model.into()),
        ..Default::default()
    };
    let resolved_model = crate::service::spawn::resolve_fresh_agent_model(
        &registry,
        provider_id,
        Some(requested_model.into()),
        None,
    )
    .expect("explicit free-form model stays selected");

    let command = build_agent_command(provider_id, &params, None, None, &registry);

    assert_eq!(resolved_model, requested_model);
    assert!(command.contains(&resolved_model), "{command}");
    assert!(!command.contains("opencode/big-pickle"), "{command}");
}

struct SpawnableChatBackend;

impl ChatBackendProvider for SpawnableChatBackend {
    fn id(&self) -> &str {
        "test-chat"
    }
    fn spawnable(&self) -> bool {
        true
    }
    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        Ok("session".into())
    }
    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }
    fn poll(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<crate::agent::chat_types::StructuredChat, ChatBackendError> {
        Ok(Default::default())
    }
    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }
    fn list_models(&self) -> Result<Vec<crate::agent::provider::ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

#[test]
fn transport_capability_paths_receive_the_primer_for_plain_orchestrator() {
    let params = orchestrator_params(None);
    let prompt = spawn_onboarding_task(&params).expect("orchestrator system prompt");
    assert!(prompt.contains(crate::prompts::orchestrator_suffix().trim()));
    match SpawnTabSpec::from_agent_params(params.clone()).command {
        TabCommand::Agent(agent) => assert_eq!(agent.task, None),
        other => panic!("expected PTY agent route, got {other:?}"),
    }
    let chatbackends = ChatBackendRegistry::new();
    chatbackends.register("test-chat", Arc::new(SpawnableChatBackend));
    let chat_params = SpawnAgentParams {
        agent: Some("test-chat".into()),
        ..params
    };
    match SpawnTabSpec::from_agent_params_with_chatbackends(&chatbackends, chat_params).command {
        TabCommand::ChatBackend { config, role, .. } => {
            assert!(config.as_object().unwrap().is_empty());
            assert_eq!(role.as_deref(), Some("Orchestrator"));
        }
        other => panic!("expected chatBackend route, got {other:?}"),
    }
}
