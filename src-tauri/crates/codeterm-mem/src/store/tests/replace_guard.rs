use super::*;

#[test]
fn write_area_guard_refuses_foreign_and_protected_targets() {
    let guard = ReplaceGuard::new(vec!["project".into()]);
    assert_eq!(guard.check_insert("project"), Ok(()));
    assert_eq!(
        guard.check_insert("private"),
        Err(ReplaceGuardError::AreaNotAllowed)
    );
    for kind in [MemKind::Fact, MemKind::Note] {
        assert_eq!(guard.check_replace("project", kind, false), Ok(()));
        assert_eq!(
            guard.check_replace("private", kind, false),
            Err(ReplaceGuardError::TitleConflict)
        );
        assert_eq!(
            guard.check_replace("project", kind, true),
            Err(ReplaceGuardError::TitleConflict)
        );
    }
    for kind in [
        MemKind::Rule,
        MemKind::Preference,
        MemKind::Skill,
        MemKind::Script,
    ] {
        assert_eq!(
            guard.check_replace("project", kind, false),
            Err(ReplaceGuardError::TitleConflict)
        );
    }
}
