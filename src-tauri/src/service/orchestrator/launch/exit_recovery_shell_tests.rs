use super::shell_prompt_already_idle;
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
fn idle_shell_prompt_after_the_provider_exit_lets_recovery_write_without_a_new_prompt() {
    let mut events = provider_ran_and_exited();
    events.push(SemanticEvent::PromptStart);
    let shell = detection(events);

    assert!(shell_prompt_already_idle(true, true, || {
        shell.prompt_idle_after_command()
    }));
}

#[test]
fn shell_that_never_showed_an_idle_prompt_keeps_the_strict_wait() {
    let mut running = provider_ran_and_exited();
    running.extend([
        SemanticEvent::PromptStart,
        command_line("other"),
        SemanticEvent::CommandStart,
    ]);
    let mut typed = provider_ran_and_exited();
    typed.extend([SemanticEvent::PromptStart, command_line("other")]);

    for shell in [
        CommandDetection::new(),
        detection(vec![SemanticEvent::PromptStart]),
        detection(provider_ran_and_exited()),
        detection(running),
        detection(typed),
    ] {
        assert!(!shell_prompt_already_idle(true, true, || {
            shell.prompt_idle_after_command()
        }));
    }
}

#[test]
fn only_exit_recovery_on_its_reused_pty_accepts_the_idle_level() {
    assert!(!shell_prompt_already_idle(false, true, || true));
    assert!(!shell_prompt_already_idle(true, false, || true));
    assert!(!shell_prompt_already_idle(true, true, || false));
}
