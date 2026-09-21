//! Native/JS parity for Claude's tool-permission approval recognition. Both
//! runtimes receive the identical prompt and screen.

use super::*;
use crate::permission_prompt::PermissionPromptResponse;
use crate::provider::AgentProvider;
use codeterm_types::pty::{PendingPrompt, PromptOption};

const APPROVAL_SCREEN: &str = "Bash command\n\n  git status --short\n  Show working tree status\n\nDo you want to proceed?\n❯ 1. Yes\n  2. Yes, and don't ask again for git status commands in /repo\n  3. No, and tell Claude what to do differently (esc)\n\nEsc to cancel";

fn prompt(question: &str, labels: &[&str]) -> PendingPrompt {
    PendingPrompt {
        question: question.into(),
        options: labels
            .iter()
            .enumerate()
            .map(|(i, label)| PromptOption {
                label: (*label).into(),
                value: (i + 1).to_string(),
            })
            .collect(),
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    }
}

fn approval() -> PendingPrompt {
    prompt(
        "Do you want to proceed?",
        &[
            "Yes",
            "Yes, and don't ask again for git status commands in /repo",
            "No, and tell Claude what to do differently (esc)",
        ],
    )
}

fn both(
    tool: &str,
    prompt: &PendingPrompt,
    screen: &str,
) -> (
    Option<PermissionPromptResponse>,
    Option<PermissionPromptResponse>,
) {
    let (native, js) = pair();
    (
        native.permission_prompt_response(tool, prompt, screen),
        js.permission_prompt_response(tool, prompt, screen),
    )
}

#[test]
fn permission_prompt_response_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = both("Bash", &approval(), APPROVAL_SCREEN);
    let native = native.expect("native recognizes the tool approval");
    assert_eq!(native.option_index, 0);
    assert_eq!(native.steps, vec![b"\r".to_vec()]);
    assert_eq!(Some(native), js);
}

#[test]
fn permission_prompt_refusals_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let trust = prompt("Trust?", &["Yes", "No"]);
    let trust_screen = "Trust?";
    let declineless = prompt("Do you want to proceed?", &["Yes", "Yes, always"]);
    let mut multi = approval();
    multi.multi_select = Some(true);

    let cases: Vec<(&str, PendingPrompt, &str)> = vec![
        ("AskUserQuestion", approval(), APPROVAL_SCREEN),
        ("ExitPlanMode", approval(), APPROVAL_SCREEN),
        ("", approval(), APPROVAL_SCREEN),
        ("Bash", approval(), "no prompt here"),
        ("Bash", trust, trust_screen),
        ("Bash", declineless, APPROVAL_SCREEN),
        ("Bash", multi, APPROVAL_SCREEN),
    ];
    for (tool, prompt, screen) in cases {
        let (native, js) = both(tool, &prompt, screen);
        assert_eq!(
            native, None,
            "native answered {tool:?} / {:?}",
            prompt.question
        );
        assert_eq!(js, None, "js answered {tool:?} / {:?}", prompt.question);
    }
}

#[test]
fn permission_auto_confirm_capability_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    assert!(native.supports_permission_auto_confirm());
    assert!(js.supports_permission_auto_confirm());
    assert!(native.describe().supports_permission_auto_confirm);
    assert!(js.describe().supports_permission_auto_confirm);
}
