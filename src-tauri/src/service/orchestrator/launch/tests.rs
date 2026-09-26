use super::startup_readiness::reused_provider_tui_owns_launch;
use super::startup_readiness::{
    input_ready_stage_after_provider_ready, resume_waits_for_first_turn,
};
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
        tab.pty_id = Some(pty_id.into());
        tab.restored_from_snapshot = true;
    }
    let harness = crate::service::test_fixture::ServiceFixture::new(mux);
    harness.service.agent_registry.register_provider(Box::new(
        MockProvider::new("resume-first-turn-fixture").with_resume_binds_on_first_user_turn(),
    ));

    assert!(resume_waits_for_first_turn(
        &harness.service,
        &tab_id,
        "resume-first-turn-fixture",
        crate::service::launch_diagnostics::AgentLaunchOrigin::RestoredSnapshot,
    ));
    assert!(!resume_waits_for_first_turn(
        &harness.service,
        &tab_id,
        "resume-first-turn-fixture",
        crate::service::launch_diagnostics::AgentLaunchOrigin::AtomicSpawn,
    ));

    let mut launch = LaunchGeneration::begin(&tab_id, pty_id.into());
    let input_ready_stage = input_ready_stage_after_provider_ready(true);
    assert_eq!(input_ready_stage, BootStage::PtyReattached);
    assert!(launch.emit(&harness.service, input_ready_stage));
    let mux = harness.service.mux.read();
    let tab = mux.get_tab(&tab_id).expect("restored tab remains present");
    assert_eq!(tab.boot_stage, Some(BootStage::PtyReattached));
    assert_eq!(tab.boot_failure_reason, None);
    drop(mux);
    LAUNCH_REGISTRY.lock().finish(&launch);
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
