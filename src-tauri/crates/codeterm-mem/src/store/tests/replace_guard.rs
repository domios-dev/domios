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

#[test]
fn skill_replacement_requires_opt_in_and_preserves_area_and_pin_guards() {
    let guard = ReplaceGuard::new(vec!["project".into()]).with_skill_saves(true);
    assert_eq!(
        guard.check_replace("project", MemKind::Skill, false),
        Ok(())
    );
    assert_eq!(
        guard.check_replace("private", MemKind::Skill, false),
        Err(ReplaceGuardError::TitleConflict)
    );
    assert_eq!(
        guard.check_replace("project", MemKind::Skill, true),
        Err(ReplaceGuardError::TitleConflict)
    );
    assert_eq!(
        guard.check_replace("project", MemKind::Rule, false),
        Err(ReplaceGuardError::TitleConflict)
    );
}
