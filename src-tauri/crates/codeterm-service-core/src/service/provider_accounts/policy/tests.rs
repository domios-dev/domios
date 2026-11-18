use super::*;
use codeterm_types::config::provider_accounts::ProviderAccountConfig;

#[test]
fn policy_changes_are_revision_fenced_and_preserve_the_implicit_namespace() {
    let mut policy = ProviderAccountsConfig {
        profiles: [("work".into(), ProviderAccountConfig::default())].into(),
        order: vec!["default".into(), "work".into()],
        ..Default::default()
    };
    let original = revision(&policy).unwrap();
    apply(
        &mut policy,
        &original,
        AccountContinuation::Unverified,
        &PolicyAction::Reorder {
            account_id: "work".into(),
            position: 0,
        },
    )
    .unwrap();
    assert_eq!(policy.starting_account(None, None).unwrap(), "work");
    assert!(apply(
        &mut policy,
        &original,
        AccountContinuation::Proven,
        &PolicyAction::Rename {
            account_id: "default".into(),
            name: "Usual".into()
        }
    )
    .is_err());
    let current = revision(&policy).unwrap();
    apply(
        &mut policy,
        &current,
        AccountContinuation::Unverified,
        &PolicyAction::Rename {
            account_id: "default".into(),
            name: "Usual".into(),
        },
    )
    .unwrap();
    assert_eq!(policy.default_name.as_deref(), Some("Usual"));
    assert!(!policy.profiles.contains_key("default"));
    assert_eq!(policy.order, vec!["work", "default"]);
}

#[test]
fn continuation_gate_and_invalid_workspace_choices_cannot_mutate_a_policy() {
    let mut policy = ProviderAccountsConfig::default();
    let original = policy.clone();
    let expected = revision(&policy).unwrap();
    assert!(apply(
        &mut policy,
        &expected,
        AccountContinuation::Unverified,
        &PolicyAction::AutomaticSwitch { enabled: true }
    )
    .is_err());
    assert_eq!(policy, original);
    assert!(apply(
        &mut policy,
        &expected,
        AccountContinuation::Proven,
        &PolicyAction::Workspace {
            workspace_id: "workspace".into(),
            account_id: Some("foreign".into())
        }
    )
    .is_err());
    assert_eq!(policy, original);
    apply(
        &mut policy,
        &expected,
        AccountContinuation::Proven,
        &PolicyAction::Workspace {
            workspace_id: "workspace".into(),
            account_id: Some("default".into()),
        },
    )
    .unwrap();
    assert_eq!(
        policy.starting_account(Some("workspace"), None).unwrap(),
        "default"
    );
}
