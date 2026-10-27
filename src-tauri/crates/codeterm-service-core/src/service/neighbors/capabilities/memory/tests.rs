use super::*;

#[test]
fn memory_areas_cannot_include_foreign_or_unassigned_items_implicitly() {
    let areas = vec!["project".into()];
    assert!(area_allowed(&areas, Some("project")));
    assert!(!area_allowed(&areas, Some("private")));
    assert!(!area_allowed(&areas, None));
    assert!(!area_allowed(&[], Some("project")));
    assert!(area_allowed(&["(none)".into()], None));
}
