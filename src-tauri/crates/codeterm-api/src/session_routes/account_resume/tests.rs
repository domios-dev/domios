use super::*;
use codeterm_storage::DeskLeafFields;
use serde_json::json;

fn leaf(provider: &str, session: &str, id: &str, namespace: Option<&str>) -> DeskLayoutNode {
    DeskLayoutNode::Leaf {
        tab_id: "saved".into(),
        cwd: None,
        fields: DeskLeafFields {
            values: json!({
                "agent_type":provider, "agent_session_id":session,
                "agent_identity":{"restorable_launch":{"account_id":id,"account_namespace":namespace}}
            }),
        },
    }
}

#[test]
fn exact_saved_account_survives_without_role_and_ignores_foreign_sessions() {
    let nodes = [
        leaf("own", "session", "work", Some("retained-root")),
        leaf("other", "session", "foreign", Some("foreign-root")),
        leaf("own", "other", "foreign", Some("foreign-root")),
    ];
    let account = from_layouts(&nodes, "own", "session").unwrap();
    assert_eq!(account.id, "work");
    assert_eq!(account.namespace.as_deref(), Some("retained-root"));
}

#[test]
fn usual_history_stays_on_implicit_account_and_conflicting_selections_refuse() {
    assert_eq!(
        from_layouts([], "own", "history").unwrap(),
        SavedAccount::default()
    );
    let nodes = [
        leaf("own", "session", "work", Some("root-one")),
        leaf("own", "session", "work", Some("root-two")),
    ];
    assert!(from_layouts(&nodes, "own", "session").is_err());
    let nodes = [
        leaf("own", "session", "work", Some("root-one")),
        leaf("own", "session", "work", Some("root-one")),
    ];
    assert_eq!(
        from_layouts(&nodes, "own", "session")
            .unwrap()
            .namespace
            .as_deref(),
        Some("root-one")
    );
}
