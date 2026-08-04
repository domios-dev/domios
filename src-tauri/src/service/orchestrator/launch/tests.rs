use super::{
    build_agent_command, command_with_binding_nonce_for_shell, replacement_pty_candidate,
    spawn_onboarding_task, take_reader_termination, worker_brief_delivery_incident, AgentRegistry,
    LaunchFailure, LaunchGeneration, LaunchRegistry, ProviderSessionBinding,
};
use crate::chatbackend::{ChatBackendError, ChatBackendProvider, ChatBackendRegistry};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{
    PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};
use crate::service::boot_stage::BootStageSink;
use crate::service::inject_verify::InjectionResult;
use crate::service::launch_delivery::{LaunchConsumptionOutcome, LaunchDeliveryGate};
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
fn worker_brief_without_ingestion_is_not_confirmed() {
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
    if matches!(consumption, Ok(LaunchConsumptionOutcome::Confirmed)) {
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
    assert_eq!(
        consumption.unwrap_err().delivery.code(),
        "submit_unconfirmed"
    );
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

    assert_eq!(consumption, LaunchConsumptionOutcome::Confirmed);
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
        true,
    );
    assert_eq!(
        command,
        "$env:CODETERM_SESSION_BINDING_NONCE='nonce''a'; Remove-Item Env:CLAUDECODE -ErrorAction SilentlyContinue; claude --resume"
    );
}

#[test]
fn posix_launch_inherits_an_escaped_binding_nonce() {
    let command = command_with_binding_nonce_for_shell(
        "unset CLAUDECODE && claude --resume",
        "nonce'a",
        false,
    );
    assert_eq!(
        command,
        "export CODETERM_SESSION_BINDING_NONCE='nonce'\"'\"'a'; unset CLAUDECODE && claude --resume"
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
fn plain_orchestrator_no_task_spawn_receives_the_system_contract() {
    let prompt =
        spawn_onboarding_task(&orchestrator_params(None)).expect("orchestrator system prompt");
    assert!(prompt.starts_with(crate::prompts::AGENT_CODETERM_PRIMER.trim()));
    assert!(prompt.contains(crate::prompts::ORCHESTRATOR_SUFFIX.trim()));
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
    mux.set_orchestrator_role(&tab_id)
        .unwrap();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(
        tab_identity_env(tab).get("CODETERM_ROLE"),
        Some(&"Orchestrator".to_string())
    );
}

fn bind(driver: &mut LaunchRegistry, launch: &LaunchGeneration, nonce: &str) {
    assert!(driver.bind_provider_session(launch, nonce.into()));
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
fn explicit_orchestrator_task_is_submitted_exactly_once() {
    let prompt = spawn_onboarding_task(&orchestrator_params(Some("inspect launch")))
        .expect("orchestrator system prompt");
    assert_eq!(prompt.matches("inspect launch").count(), 1);
    assert_eq!(
        prompt
            .matches(crate::prompts::AGENT_CODETERM_PRIMER.trim())
            .count(),
        1
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

#[test]
fn planner_onboarding_remains_work_when_a_task_is_provided() {
    let params = SpawnAgentParams {
        task: Some("plan this".into()),
        role: Some("Manager".into()),
        role_profile: Some("planner".into()),
        ..Default::default()
    };
    assert!(spawn_onboarding_task(&params)
        .unwrap()
        .contains(&crate::prompts::planner_suffix()));
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
    assert!(prompt.contains(crate::prompts::ORCHESTRATOR_SUFFIX.trim()));
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

/// The delivery half of the AgentPad invariant. The pre-existing prompt test
/// only forbade duplication, so a recipient that never received the rule was
/// invisible — which is exactly how spawned agents went without it.
#[test]
fn every_spawned_recipient_receives_the_agent_pad_rule_exactly_once() {
    let rule = crate::prompts::AGENT_PAD_RULE.trim();

    for (recipient, composed) in [
        ("spawned non-worker", super::agent_system_prompt(None)),
        ("spawned worker", super::worker_agent_system_prompt(None)),
        (
            "spawned non-worker with assignment",
            super::agent_system_prompt(Some("do the thing")),
        ),
        (
            "spawned worker with assignment",
            super::worker_agent_system_prompt(Some("do the thing")),
        ),
    ] {
        assert_eq!(
            composed.matches(rule).count(),
            1,
            "{recipient} must receive the AgentPad rule exactly once"
        );
    }
}

/// A Worker is the recipient that had neither the rule nor the pointer to the
/// skill that carries the full reference.
#[test]
fn spawned_worker_receives_the_codeterm_primer() {
    let composed = super::worker_agent_system_prompt(Some("do the thing"));
    assert!(composed.contains(crate::prompts::AGENT_CODETERM_PRIMER.trim()));
    assert!(composed.contains("provider skill named `codeterm`"));
}

/// Rich-rendering guidance is a delivery fact. Two tests in prompts.rs were named
/// for the primer and the worker context but asserted phrases inside
/// RICH_RENDERING_GUIDANCE itself, so they proved nothing about either recipient.
#[test]
fn every_spawned_recipient_receives_rich_rendering_guidance() {
    let guidance = crate::prompts::RICH_RENDERING_GUIDANCE.trim();

    for (recipient, composed) in [
        ("spawned non-worker", super::agent_system_prompt(None)),
        ("spawned worker", super::worker_agent_system_prompt(None)),
    ] {
        assert_eq!(
            composed.matches(guidance).count(),
            1,
            "{recipient} must receive the rich-rendering guidance exactly once"
        );
    }
}
