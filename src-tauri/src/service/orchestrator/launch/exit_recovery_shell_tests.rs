use super::exited_provider_recovery_has_shell_ready_evidence;
use crate::service::launch_diagnostics::AgentLaunchOrigin;
use crate::agent::command_detection::CommandDetection;
use crate::agent::semantic_marker::SemanticEvent;

fn detection(events: Vec<SemanticEvent>) -> CommandDetection {
    let mut detection = CommandDetection::new();
    for event in events {
        detection.apply(event);
    }
    detection
}

fn command_line(text: &str) -> SemanticEvent {
    SemanticEvent::CommandLine {
        text: text.into(),
        nonce: None,
    }
}

fn provider_ran_and_exited() -> Vec<SemanticEvent> {
    vec![
        SemanticEvent::PromptStart,
        SemanticEvent::PromptEnd,
        command_line("provider --resume session"),
        SemanticEvent::CommandStart,
        SemanticEvent::CommandEnd { exit_code: Some(0) },
    ]
}

#[test]
fn finished_provider_exit_proves_shell_ready_without_a_new_prompt_marker() {
    let shell = detection(provider_ran_and_exited());
    assert!(!shell.prompt_idle_after_command());
    assert!(exited_provider_recovery_has_shell_ready_evidence(
        AgentLaunchOrigin::AutomaticExitRecovery,
        true,
        || panic!("the finished exit event already proves shell readiness"),
    ));
    assert!(!exited_provider_recovery_has_shell_ready_evidence(
        AgentLaunchOrigin::ExitRecovery,
        true,
        || false,
    ));
    assert!(exited_provider_recovery_has_shell_ready_evidence(
        AgentLaunchOrigin::ExitRecovery,
        true,
        || true,
    ));
}

#[test]
fn shell_ready_evidence_is_scoped_to_a_proven_exit_and_reused_pty() {
    assert!(!exited_provider_recovery_has_shell_ready_evidence(
        AgentLaunchOrigin::VisibleSpawn,
        true,
        || true,
    ));
    assert!(!exited_provider_recovery_has_shell_ready_evidence(
        AgentLaunchOrigin::AutomaticExitRecovery,
        false,
        || true,
    ));
}
