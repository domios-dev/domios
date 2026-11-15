use super::*;
use crate::repositories::desk::{
    records::LEAF_VALIDATIONS, tests::desk, DeskLayoutNode, DeskLeafFields, MAX_DESK_WORKSPACES,
};
use crate::repositories::workspaces::WorkspaceId;

fn visits() -> usize {
    LEAF_VALIDATIONS.with(|count| count.get())
}

#[test]
fn complete_delta_validates_each_leaf_once_including_cost_accounting() {
    for reduce in [false, true] {
        let current = desk();
        let mut incoming = current.clone();
        incoming.workspaces[0].tabs[0].title = "Changed".into();
        let before = visits();
        let validated = ValidatedInventory::new(incoming.clone()).unwrap();
        assert_eq!(visits() - before, 2);
        let submitted = visits();
        let target = validated.target(&current, reduce).unwrap();
        assert_eq!(target, incoming);
        assert_eq!(visits(), submitted);
    }
}

#[test]
fn invalid_incoming_leaf_is_rejected_before_writer_admission() {
    let mut incoming = desk();
    incoming.workspaces[0].tabs[0].layout = DeskLayoutNode::Leaf {
        tab_id: "tab-a".into(),
        cwd: None,
        fields: DeskLeafFields {
            values: serde_json::json!({"foreign_field": true}),
        },
    };
    assert!(ValidatedInventory::new(incoming).is_err());
}

#[test]
fn retained_rows_are_validated_in_the_merged_target() {
    let incoming = desk();
    let mut current = incoming.clone();
    let mut retained = current.workspaces[0].tabs[0].clone();
    retained.id = "retained".into();
    retained.layout = DeskLayoutNode::Leaf {
        tab_id: "retained".into(),
        cwd: None,
        fields: DeskLeafFields {
            values: serde_json::json!({"foreign_field": true}),
        },
    };
    current.workspaces[0].tabs.push(retained);
    assert!(ValidatedInventory::new(incoming.clone())
        .unwrap()
        .target(&current, false)
        .is_err());
    assert_eq!(
        ValidatedInventory::new(incoming.clone())
            .unwrap()
            .target(&current, true)
            .unwrap(),
        incoming
    );
}

#[test]
fn preservation_cannot_exceed_the_combined_inventory_bound() {
    let mut current = desk();
    current.workspaces[0].tabs.clear();
    current.teams.clear();
    current.agent_session_identities.clear();
    let template = current.workspaces[0].clone();
    for index in 1..MAX_DESK_WORKSPACES {
        let mut workspace = template.clone();
        workspace.record.id = WorkspaceId::new(format!("retained-{index}")).unwrap();
        current.workspaces.push(workspace);
    }
    assert!(current.validate().is_ok());
    let mut incoming = desk();
    incoming.workspaces[0].record.id = WorkspaceId::new("incoming").unwrap();
    incoming.teams[0].workspace_id = "incoming".into();
    assert!(ValidatedInventory::new(incoming)
        .unwrap()
        .target(&current, false)
        .is_err());
}
