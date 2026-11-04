use super::*;

fn entry(role: &str, id: i64) -> RefEntry {
    RefEntry {
        reference: format!("e{id}"),
        role: role.into(),
        name: "Search".into(),
        backend_node_id: id,
    }
}

#[test]
fn intent_prefers_input_or_clickable_roles_without_guessing_between_inputs() {
    let entries = vec![entry("link", 1), entry("textbox", 2)];
    assert_eq!(
        choose(entries.clone(), true, false)
            .unwrap()
            .backend_node_id,
        2
    );
    assert_eq!(choose(entries, false, false).unwrap().backend_node_id, 1);
    let error = choose(
        vec![entry("textbox", 2), entry("searchbox", 3)],
        true,
        false,
    )
    .unwrap_err();
    assert!(error.contains("@e2") && error.contains("@e3"));
}

#[test]
fn visible_duplicate_choice_requires_exactly_one_visible_match() {
    assert_eq!(
        choose(vec![entry("textbox", 2)], true, true)
            .unwrap()
            .backend_node_id,
        2
    );
    assert!(choose(vec![], true, true)
        .unwrap_err()
        .starts_with("hidden:"));
    assert!(
        choose(vec![entry("textbox", 2), entry("textbox", 3)], true, true)
            .unwrap_err()
            .starts_with("ambiguous:")
    );
}

#[test]
fn explicit_role_and_exact_name_precede_fuzzy_matching() {
    let mut snapshot = Snapshot::default();
    snapshot.index = vec![entry("link", 1), entry("textbox", 2), entry("textbox", 3)];
    snapshot.index[2].name = "Search articles".into();
    assert_eq!(
        candidates(&snapshot, &["textbox".into(), "search".into()]).unwrap(),
        vec![entry("textbox", 2)]
    );
    assert_eq!(candidates(&snapshot, &["Search".into()]).unwrap().len(), 2);
    assert!(candidates(&snapshot, &["@old-e1".into()]).is_err());
}
