use super::*;

#[test]
fn ease_missing_work_tag_is_inferred_without_replacing_explicit_values() {
    for (area, expected) in [
        ("prod-docs/API.md", "docs"),
        ("src/App.tsx", "frontend"),
        ("src-tauri/lib.rs", "backend"),
        ("build.sh", "infra"),
        ("tests/fixture", "qa"),
        ("figma", "design"),
        ("unknown", "backend"),
    ] {
        let mut tags = vec!["urgent".into()];
        assert_eq!(
            infer_work_tag(["work"], &mut tags, area),
            Some(format!("work:{expected}"))
        );
        validate_work_facet(["work"], tags.iter().map(String::as_str)).unwrap();
    }
    let mut tags = vec!["work:invalid".into()];
    assert_eq!(infer_work_tag(["work"], &mut tags, "src/App.tsx"), None);
    assert!(validate_work_facet(["work"], tags.iter().map(String::as_str)).is_err());
    let mut tags = Vec::new();
    assert_eq!(infer_work_tag(["area"], &mut tags, "src/App.tsx"), None);
    assert!(tags.is_empty());
}
