use super::*;
use crate::agent::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};
use crate::multiplexer::state::MultiplexerState;
use crate::service::worker_ack::AgentStartupAcknowledgement;
use codeterm_types::pty::PendingPrompt;
use std::path::PathBuf;
use std::time::Duration;

struct StartupPromptProvider;

impl AgentProvider for StartupPromptProvider {
    fn id(&self) -> &str {
        "startup-prompt-test"
    }

    fn display_name(&self) -> &str {
        "Startup Prompt Test"
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        "startup-prompt-test".into()
    }

    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::InlineFlag
    }

    fn detect_from_title(&self, _: &str) -> bool {
        false
    }

    fn detect_from_output(&self, _: &str) -> bool {
        false
    }

    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }

    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }

    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }

    fn parse_directory_trust_prompt(&self, screen: &str) -> Option<PendingPrompt> {
        screen.contains("trust-prompt").then(|| PendingPrompt {
            question: "Trust this directory?".into(),
            options: Vec::new(),
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        })
    }

    fn parse_prompt(&self, screen: &str) -> Option<PendingPrompt> {
        screen.contains("permission-prompt").then(|| PendingPrompt {
            question: "Allow this action?".into(),
            options: Vec::new(),
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        })
    }
}

fn prompt_fixture() -> (crate::service::test_fixture::ServiceFixture, String, String) {
    let (mut mux, parent_tab_id) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&parent_tab_id).unwrap();
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("startup-prompt-test".into());
    tab.pty_id = Some("launch-a".into());
    tab.session_binding_nonce = Some("nonce-a".into());
    tab.cwd = Some(r"C:\untrusted\repo".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(StartupPromptProvider));
    (fixture, parent_tab_id, tab_id)
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn startup_ack_with_nothing_armed_reports_not_required() {
    let (fixture, _, tab_id) = prompt_fixture();

    let acknowledgement = fixture
        .service
        .acknowledge_agent_startup(&tab_id, "launch-a", "nonce-a")
        .expect("an unsupervised startup ACK is harmless");

    assert_eq!(acknowledgement, AgentStartupAcknowledgement::NotRequired);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn startup_ack_with_an_armed_attempt_still_acknowledges() {
    let (fixture, parent_tab_id, tab_id) = prompt_fixture();
    let authority =
        StartupAuthority::new(&parent_tab_id, &tab_id, "launch-a").for_binding(None, "nonce-a");
    let receiver = startup_receiver(&fixture.service.worker_ack, authority);

    let acknowledgement = fixture
        .service
        .acknowledge_agent_startup(&tab_id, "launch-a", "nonce-a")
        .expect("an armed startup ACK remains accepted");

    assert_eq!(acknowledgement, AgentStartupAcknowledgement::Acknowledged);
    assert_eq!(receiver.recv().unwrap(), AgentStartupSignal::Ready);
    assert_eq!(
        fixture
            .service
            .acknowledge_agent_startup(&tab_id, "launch-a", "nonce-a")
            .expect("a repeated ACK remains an explicit success"),
        AgentStartupAcknowledgement::AlreadyAcknowledged
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn task_file_team_spawn_handshake_is_injected_only_for_an_armed_attempt() {
    let (fixture, parent_tab_id, tab_id) = prompt_fixture();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&parent_tab_id)
        .unwrap()
        .agent_type = Some("startup-prompt-test".into());
    {
        let mut mux = fixture.service.mux.write();
        let child = mux.get_tab_mut(&tab_id).unwrap();
        child.parent_tab_id = Some(parent_tab_id.clone());
        child.team_id = Some("team-a".into());
    }

    let supervised = fixture
        .service
        .prepare_agent_startup_supervision(&tab_id, Some(&parent_tab_id), None);
    assert!(supervised);
    let handshake = crate::service::launch_delivery::startup_starter_prompt(supervised, false);
    assert_eq!(
        handshake,
        Some(crate::agent::provider::StarterPrompt::StartupHandshake)
    );

    fixture
        .service
        .activate_agent_startup_supervision(&tab_id, "launch-a", "nonce-a", None)
        .unwrap();
    assert_eq!(fixture.service.worker_ack.startup_waiters.lock().len(), 1);
    assert_eq!(
        fixture
            .service
            .acknowledge_agent_startup(&tab_id, "launch-a", "nonce-a")
            .unwrap(),
        AgentStartupAcknowledgement::Acknowledged
    );

    assert_eq!(
        crate::service::launch_delivery::startup_starter_prompt(false, false),
        None
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn startup_ack_after_binding_auto_ack_reports_already_acknowledged() {
    let (fixture, parent_tab_id, tab_id) = prompt_fixture();
    let authority =
        StartupAuthority::new(&parent_tab_id, &tab_id, "launch-a").for_binding(None, "nonce-a");
    let _receiver = startup_receiver(&fixture.service.worker_ack, authority);

    assert_eq!(
        fixture
            .service
            .worker_ack
            .resolve_startup_ack_for_binding(&tab_id, "launch-a", None, "nonce-a",)
            .expect("the session binding acknowledges its launch"),
        AgentStartupAcknowledgement::Acknowledged
    );
    assert_eq!(
        fixture
            .service
            .acknowledge_agent_startup(&tab_id, "launch-a", "nonce-a")
            .expect("the injected instruction succeeds after binding auto-ack"),
        AgentStartupAcknowledgement::AlreadyAcknowledged
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn startup_ack_from_a_superseded_attempt_keeps_its_explicit_refusal() {
    let (fixture, parent_tab_id, tab_id) = prompt_fixture();
    let authority =
        StartupAuthority::new(&parent_tab_id, &tab_id, "launch-b").for_binding(None, "nonce-a");
    let _receiver = startup_receiver(&fixture.service.worker_ack, authority);

    let refusal = fixture
        .service
        .acknowledge_agent_startup(&tab_id, "launch-a", "nonce-a")
        .expect_err("a superseded startup ACK remains refused");

    assert!(refusal.contains("superseded by launch-b"));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn finalized_startup_attempt_still_refuses_its_acknowledgement() {
    let (fixture, parent_tab_id, tab_id) = prompt_fixture();
    let authority =
        StartupAuthority::new(&parent_tab_id, &tab_id, "launch-a").for_binding(None, "nonce-a");
    let receiver = startup_receiver(&fixture.service.worker_ack, authority.clone());
    assert_eq!(
        fixture
            .service
            .worker_ack
            .expire_startup_generation(&authority, 0),
        WaiterExpiry::Expired
    );
    drop(receiver);

    let refusal = fixture
        .service
        .acknowledge_agent_startup(&tab_id, "launch-a", "nonce-a")
        .expect_err("a finalized attempt cannot accept a stale startup ACK");

    assert!(refusal.contains("finalized"));
    assert!(refusal.contains("not awaiting acknowledgement"));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn closing_a_tab_removes_its_finalized_startup_authority() {
    let (fixture, parent_tab_id, tab_id) = prompt_fixture();
    let authority =
        StartupAuthority::new(&parent_tab_id, &tab_id, "launch-a").for_binding(None, "nonce-a");
    let receiver = startup_receiver(&fixture.service.worker_ack, authority.clone());
    assert_eq!(
        fixture
            .service
            .worker_ack
            .expire_startup_generation(&authority, 0),
        WaiterExpiry::Expired
    );
    drop(receiver);
    assert!(fixture
        .service
        .worker_ack
        .finalized_startups
        .lock()
        .contains_key(&tab_id));

    fixture
        .service
        .close_tab(&tab_id)
        .expect("normal tab close succeeds");

    assert!(!fixture
        .service
        .worker_ack
        .finalized_startups
        .lock()
        .contains_key(&tab_id));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn closing_a_tab_cancels_its_live_startup_waiter() {
    let (fixture, parent_tab_id, tab_id) = prompt_fixture();
    let authority =
        StartupAuthority::new(&parent_tab_id, &tab_id, "launch-a").for_binding(None, "nonce-a");
    let receiver = startup_receiver(&fixture.service.worker_ack, authority);

    fixture
        .service
        .close_tab(&tab_id)
        .expect("normal tab close succeeds");

    assert_eq!(receiver.recv().unwrap(), AgentStartupSignal::Superseded);
    assert!(!fixture
        .service
        .worker_ack
        .startup_waiters
        .lock()
        .contains_key(&tab_id));
    assert!(!fixture
        .service
        .worker_ack
        .finalized_startups
        .lock()
        .contains_key(&tab_id));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn startup_deadline_on_a_trust_prompt_records_the_classified_reason() {
    let (fixture, parent_tab_id, tab_id) = prompt_fixture();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.lifecycle_epoch = Some(7);
        tab.lifecycle_operation_id = Some("startup-lifecycle".into());
    }
    fixture
        .service
        .screen_state
        .lock()
        .process("launch-a", b"trust-prompt");
    let authority =
        StartupAuthority::new(&parent_tab_id, &tab_id, "launch-a").for_binding(None, "nonce-a");

    let payload = fixture
        .service
        .agent_startup_failure_payload(&authority, "Worker", Some(1))
        .expect("the current startup attempt remains reportable");
    let reason = "blocked on provider directory-trust prompt; no provenance to auto-answer; no session binding";

    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.boot_failure_reason.as_deref()),
        Some(reason)
    );
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.boot_stage, None);
        assert_eq!(tab.lifecycle_epoch, Some(7));
        assert_eq!(
            tab.lifecycle_operation_id.as_deref(),
            Some("startup-lifecycle")
        );
    }
    assert_eq!(payload["kind"], "agent_startup_failed");
    assert_eq!(payload["failure_reason"], reason);
    let notice = worker_lifecycle_notice("Worker", Some("test"), None, &payload);
    assert_eq!(notice.lines().count(), 1);
    assert!(notice.contains(reason));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn startup_prompt_classification_requires_missing_session_binding() {
    let (fixture, _, tab_id) = prompt_fixture();
    fixture
        .service
        .screen_state
        .lock()
        .process("launch-a", b"permission-prompt");

    assert_eq!(
        fixture
            .service
            .startup_prompt_failure_reason(&tab_id, "launch-a")
            .as_deref(),
        Some("blocked on provider permission prompt; no session binding")
    );
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("session-a".into());
    }
    assert_eq!(
        fixture
            .service
            .startup_prompt_failure_reason(&tab_id, "launch-a")
            .as_deref(),
        Some("blocked on provider permission prompt; no session binding")
    );
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.session_binding_nonce = Some("nonce-a".into());
        tab.reported_session_binding_nonce = Some("nonce-a".into());
    }

    assert_eq!(
        fixture
            .service
            .startup_prompt_failure_reason(&tab_id, "launch-a"),
        None
    );
}
