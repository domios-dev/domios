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

fn host_tab() -> AccessFacts<'static> {
    AccessFacts {
        host_approvals: true,
        ..Default::default()
    }
}

#[test]
fn host_approvals_default_to_no_questions_whatever_the_launch_recorded() {
    for recorded in [false, true] {
        assert!(full_access_in_effect(AccessFacts {
            recorded,
            ..host_tab()
        }));
    }
}

#[test]
fn the_owners_host_choice_decides_and_outranks_the_launch_record() {
    assert!(!full_access_in_effect(AccessFacts {
        recorded: true,
        choice: Some(HostApprovalChoice::Ask),
        ..host_tab()
    }));
    assert!(full_access_in_effect(AccessFacts {
        recorded: false,
        choice: Some(HostApprovalChoice::AllowAll),
        ..host_tab()
    }));
}

#[test]
fn profiles_without_host_approvals_and_grant_bound_tabs_keep_their_launch_record() {
    for recorded in [false, true] {
        assert_eq!(
            full_access_in_effect(AccessFacts {
                recorded,
                choice: Some(HostApprovalChoice::AllowAll),
                selected_mode: Some("opaque-approve-all"),
                native_mode: Some("opaque-approve-all"),
                ..Default::default()
            }),
            recorded
        );
        assert_eq!(
            full_access_in_effect(AccessFacts {
                recorded,
                grant_bound: true,
                ..host_tab()
            }),
            recorded
        );
    }
}

const NATIVE: AccessFacts<'static> = AccessFacts {
    recorded: false,
    host_approvals: true,
    grant_bound: false,
    native_mode: Some("opaque-approve-all"),
    native_advertised: Some(true),
    current_mode: Some("opaque-default"),
    selected_mode: None,
    choice: None,
};

#[test]
fn a_chosen_approve_all_mode_answers_questions_the_agent_still_asks_on_an_interactive_tab() {
    assert!(full_access_in_effect(AccessFacts {
        current_mode: Some("opaque-approve-all"),
        selected_mode: Some("opaque-approve-all"),
        ..NATIVE
    }));
}

#[test]
fn a_chosen_asking_mode_still_asks_even_on_a_full_access_launch() {
    assert!(!full_access_in_effect(AccessFacts {
        recorded: true,
        selected_mode: Some("opaque-default"),
        ..NATIVE
    }));
}

#[test]
fn the_live_mode_decides_once_the_owner_has_chosen_one() {
    assert!(!full_access_in_effect(AccessFacts {
        selected_mode: Some("opaque-approve-all"),
        current_mode: Some("opaque-plan"),
        ..NATIVE
    }));
}

#[test]
fn before_a_session_reports_modes_the_selection_decides_the_launch() {
    let launch = AccessFacts {
        native_advertised: None,
        current_mode: None,
        ..NATIVE
    };
    assert!(full_access_in_effect(AccessFacts {
        selected_mode: Some("opaque-approve-all"),
        ..launch
    }));
    assert!(!full_access_in_effect(AccessFacts {
        selected_mode: Some("opaque-default"),
        ..launch
    }));
    assert!(full_access_in_effect(launch));
}

#[test]
fn with_no_mode_chosen_the_host_backstops_an_agent_that_refused_its_approve_all_mode() {
    assert!(full_access_in_effect(NATIVE));
}

#[test]
fn a_declared_mode_the_agent_does_not_offer_leaves_the_host_choice_in_charge() {
    let unoffered = AccessFacts {
        native_advertised: Some(false),
        selected_mode: Some("opaque-default"),
        ..NATIVE
    };
    assert!(full_access_in_effect(unoffered));
    assert!(!full_access_in_effect(AccessFacts {
        choice: Some(HostApprovalChoice::Ask),
        ..unoffered
    }));
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
