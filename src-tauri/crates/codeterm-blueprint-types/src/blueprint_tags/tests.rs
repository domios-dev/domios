use super::*;

#[test]
fn controlled_vocabulary_is_unique_and_described() {
    assert!((40..=60).contains(&vocabulary().len()));
    let mut names = std::collections::BTreeSet::new();
    for tag in vocabulary() {
        assert!(names.insert(&tag.name));
        assert!(!tag.description.trim().is_empty());
        assert!(tag
            .name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c == b'-'));
    }
}

#[test]
fn keep_tags_require_one_to_three_distinct_controlled_values() {
    for tags in [
        vec!["ci".into()],
        vec!["ci".into(), "github".into(), "release".into()],
    ] {
        assert_eq!(validate(&tags).unwrap(), tags);
    }
    for tags in [
        vec![],
        vec!["ci".into(); 4],
        vec!["ci".into(), "ci".into()],
        vec!["CI".into()],
    ] {
        assert_eq!(validate(&tags).unwrap_err().code, "blueprint_tags_invalid");
    }
    let error = validate(&["githb".into()]).unwrap_err();
    assert_eq!(error.suggestions[0], "github");
    assert!(error.to_string().contains("closest valid tags"));
}
