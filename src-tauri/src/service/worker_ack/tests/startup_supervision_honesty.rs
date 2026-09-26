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
fn startup_deadline_on_a_trust_prompt_records_the_classified_reason() {
    let (fixture, parent_tab_id, tab_id) = prompt_fixture();
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
