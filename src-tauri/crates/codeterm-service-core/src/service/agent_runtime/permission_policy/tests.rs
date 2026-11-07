use super::*;
use codeterm_plugin::agent_runtime::RuntimePermissionOption;
use codeterm_types::pty::PromptOption;
use RuntimePermissionAction::{Command, Other, Read, Write};
use RuntimePermissionDecision::{AllowAlways, AllowOnce, RejectAlways, RejectOnce};

fn request(
    action: RuntimePermissionAction,
    declared: &[(&str, RuntimePermissionDecision)],
) -> (RuntimePermission, PendingPrompt) {
    let permission = RuntimePermission {
        action,
        options: declared
            .iter()
            .map(|(value, decision)| RuntimePermissionOption {
                value: (*value).into(),
                decision: *decision,
            })
            .collect(),
    };
    let prompt = PendingPrompt {
        request_id: None,
        question: "opaque".into(),
        options: declared
            .iter()
            .map(|(value, _)| PromptOption {
                label: format!("label-{value}"),
                value: (*value).into(),
            })
            .collect(),
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    };
    (permission, prompt)
}

const ALL: &[(&str, RuntimePermissionDecision)] = &[
    ("o1", AllowOnce),
    ("o2", AllowAlways),
    ("o3", RejectOnce),
    ("o4", RejectAlways),
];

fn decided(
    access: Access,
    action: RuntimePermissionAction,
    declared: &[(&str, RuntimePermissionDecision)],
) -> Option<String> {
    let (permission, prompt) = request(action, declared);
    decide(access, &permission, &prompt)
}

#[test]
fn full_access_answers_every_action_with_the_declared_allow_option() {
    for action in [Read, Write, Command, Other] {
        assert_eq!(decided(Access::Full, action, ALL).as_deref(), Some("o2"));
        assert_eq!(
            decided(
                Access::Full,
                action,
                &[("o1", AllowOnce), ("o3", RejectOnce)]
            )
            .as_deref(),
            Some("o1")
        );
        assert_eq!(decided(Access::Full, action, &[("o3", RejectOnce)]), None);
    }
}

#[test]
fn read_only_rejects_writes_commands_and_unknown_actions() {
    for action in [Write, Command, Other] {
        assert_eq!(
            decided(Access::ReadOnly, action, ALL).as_deref(),
            Some("o3")
        );
        assert_eq!(
            decided(
                Access::ReadOnly,
                action,
                &[("o1", AllowOnce), ("o4", RejectAlways)]
            )
            .as_deref(),
            Some("o4")
        );
        assert_eq!(
            decided(Access::ReadOnly, action, &[("o1", AllowOnce)]),
            None
        );
    }
    assert_eq!(decided(Access::ReadOnly, Read, ALL), None);
}

#[test]
fn workspace_sandbox_refuses_commands_and_never_escalates_writes() {
    assert_eq!(
        decided(Access::WorkspaceWrite, Command, ALL).as_deref(),
        Some("o3")
    );
    assert_eq!(
        decided(Access::WorkspaceWrite, Other, ALL).as_deref(),
        Some("o3")
    );
    assert_eq!(decided(Access::WorkspaceWrite, Write, ALL), None);
    assert_eq!(decided(Access::WorkspaceWrite, Read, ALL), None);
}

#[test]
fn interactive_tabs_always_ask() {
    for action in [Read, Write, Command, Other] {
        assert_eq!(decided(Access::Interactive, action, ALL), None);
    }
}

#[test]
fn declared_values_absent_from_the_prompt_are_never_chosen() {
    let (permission, mut prompt) = request(Write, ALL);
    prompt.options.retain(|option| option.value != "o2");
    assert_eq!(
        decide(Access::Full, &permission, &prompt).as_deref(),
        Some("o1")
    );
    prompt.options.clear();
    assert_eq!(decide(Access::Full, &permission, &prompt), None);
}

#[test]
fn only_declared_reject_choices_count_as_declined() {
    let (permission, _) = request(Write, ALL);
    let values = |values: &[&str]| {
        values
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>()
    };
    assert!(rejects(&permission, &values(&["o3"])));
    assert!(rejects(&permission, &values(&["o4"])));
    assert!(!rejects(&permission, &values(&["o1"])));
    assert!(!rejects(&permission, &values(&["o3", "o2"])));
    assert!(!rejects(&permission, &values(&["undeclared"])));
    assert!(!rejects(&permission, &[]));
}
