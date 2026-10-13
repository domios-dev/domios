//! Native/JS parity for Claude's tool-permission approval recognition. Both
//! runtimes receive the identical prompt and screen.

use super::*;
use crate::permission_prompt::PermissionPromptResponse;
use crate::provider::AgentProvider;
use codeterm_types::pty::{PendingPrompt, PromptOption};

const APPROVAL_SCREEN: &str = "Bash command\n\n  git status --short\n  Show working tree status\n\nDo you want to proceed?\n❯ 1. Yes\n  2. Yes, and don't ask again for git status commands in /repo\n  3. No, and tell Claude what to do differently (esc)\n\nEsc to cancel";

fn prompt(question: &str, labels: &[&str]) -> PendingPrompt {
    PendingPrompt {
        request_id: None,
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
fn rendered_claude_prompt_options_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let fixtures = workspace_plugins_root()
        .join("provider-claude")
        .join("tests/fixtures");
    for file in [
        "folder-trust/tool-permission.txt",
        "folder-trust/mcp-server.txt",
        "wizard-multi-step.txt",
    ] {
        let screen = std::fs::read_to_string(fixtures.join(file)).expect("Claude screen fixture");
        assert_eq!(
            native.parse_prompt(&screen),
            js.parse_prompt(&screen),
            "rendered option parity for {file}"
        );
    }
    let permission_screen =
        std::fs::read_to_string(fixtures.join("folder-trust/tool-permission.txt"))
            .expect("Claude permission screen fixture");
    let parsed_permission = native
        .parse_prompt(&permission_screen)
        .expect("rendered permission prompt parses");
    assert_eq!(
        parsed_permission
            .options
            .iter()
            .map(|option| option.label.as_str())
            .collect::<Vec<_>>(),
        [
            "Yes",
            "Yes, and don't ask again for git status commands in /repo",
            "No, and tell Claude what to do differently (esc)",
        ]
    );

    let multi_select = "← ✔ Environment ☐ Focus ✔ Submit →\nWhich focus areas should be enabled?\n❯ ☐ Performance\n  ☑ Tests\n  ☐ Docs\n  5. Submit\n  6. Chat about this\nEnter to select · Esc to cancel";
    let native_prompt = native
        .parse_prompt(multi_select)
        .expect("multi-select screen parses");
    assert_eq!(
        native_prompt
            .options
            .iter()
            .map(|option| option.label.as_str())
            .collect::<Vec<_>>(),
        ["Performance", "Tests", "Docs", "Submit", "Chat about this"]
    );
    assert_eq!(native_prompt.multi_select, Some(true));
    assert_eq!(native_prompt.submit_available, Some(true));
    assert_eq!(native_prompt.selected_values, Some(vec!["2".to_string()]));
    assert_eq!(
        native.parse_prompt(multi_select),
        js.parse_prompt(multi_select)
    );
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
