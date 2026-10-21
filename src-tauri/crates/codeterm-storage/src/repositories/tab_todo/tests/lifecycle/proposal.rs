use super::*;
use crate::repositories::tab_todo::{
    TabTodoAmendmentRecord, TabTodoOwnWriteBase, TabTodoOwnWriteRecord,
};

#[test]
fn proposal_persists_across_reopen_and_carry_over() {
    let (root, storage, repository) = open_repository();
    let current = seed(&repository, "source", &["criterion"]);
    let json = r#"{"state":"proposed","shared_at":100,"reviewed_at":null,"reviewer_tab_id":null,"note":null}"#;
    let shared = wait(repository.review_proposal(
        "source".into(),
        current.generation,
        current.revision,
        json.into(),
        vec![],
        None,
        Some("source".into()),
        100,
    ))
    .value;
    assert_eq!(shared.proposal_json.as_deref(), Some(json));
    assert!(outcome(repository.review_proposal(
        "source".into(),
        current.generation,
        current.revision,
        json.into(),
        vec![],
        None,
        None,
        100
    ))
    .is_err());
    wait(storage.shutdown());
    let reopened = Storage::open(root.path(), StorageConfig::default()).unwrap();
    let repository = reopened.tab_todos();
    assert_eq!(
        wait(repository.get("source".into()))
            .value
            .unwrap()
            .proposal_json
            .as_deref(),
        Some(json)
    );
    let moved = wait(repository.transfer(
        "source".into(),
        "replacement".into(),
        "workspace".into(),
        Some("team".into()),
        None,
        101,
    ))
    .value
    .unwrap();
    assert_eq!(moved.proposal_json.as_deref(), Some(json));
    wait(reopened.shutdown());
}

#[test]
fn failed_criterion_amendment_rolls_back_all_earlier_changes() {
    let (_root, storage, repository) = open_repository();
    let seeded = seed(&repository, "tab", &["criterion"]);
    let current = wait(repository.write_own(
        "tab".into(),
        "workspace".into(),
        Some("team".into()),
        101,
        TabTodoOwnWriteRecord {
            base: TabTodoOwnWriteBase::Edit,
            generation: seeded.generation,
            removals: vec![],
            appends: vec![item("own", "Inspect")],
            state_changes: vec![],
        },
    ))
    .value;
    let amendment = |title: &str| TabTodoAmendmentRecord {
        title: title.into(),
        description: None,
        link_json: None,
    };
    let result = outcome(repository.review_proposal(
        "tab".into(),
        current.generation,
        current.revision,
        "{}".into(),
        vec![
            ("own".into(), 1, amendment("Inspect errors")),
            ("criterion".into(), 1, amendment("Changed criterion")),
        ],
        None,
        Some("parent".into()),
        102,
    ));
    assert!(result.is_err());
    assert_eq!(wait(repository.get("tab".into())).value.unwrap(), current);
    wait(storage.shutdown());
}
