use super::*;

#[test]
fn legacy_classification_accepts_scratch_and_empty_trees_only() {
    for (name, kind, empty_tree, expected) in [
        ("logs", DestinationEntryKind::Directory, false, true),
        ("runtime", DestinationEntryKind::Directory, false, true),
        ("storage", DestinationEntryKind::Directory, true, true),
        ("storage", DestinationEntryKind::Directory, false, false),
        ("config.toml", DestinationEntryKind::File, false, false),
        ("logs", DestinationEntryKind::Other, false, false),
        ("runtime", DestinationEntryKind::File, false, false),
    ] {
        assert_eq!(
            recreatable(&[Entry {
                name: name.into(),
                kind,
                bytes: 0,
                empty_tree
            }]),
            expected,
            "{name} {kind:?}"
        );
    }
    assert!(recreatable(&[]));
}
