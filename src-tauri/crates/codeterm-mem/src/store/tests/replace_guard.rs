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
fn scoped_replace_guard_restores_after_nested_calls_and_unwind() {
    assert!(current_replace_guard().is_none());
    with_replace_guard(ReplaceGuard::new(vec!["outer".into()]), || {
        let _ = std::panic::catch_unwind(|| {
            with_replace_guard(ReplaceGuard::new(vec!["inner".into()]), || panic!("opaque"))
        });
        assert_eq!(
            current_replace_guard().unwrap().allowed_areas,
            vec!["outer"]
        );
    });
    assert!(current_replace_guard().is_none());
}
