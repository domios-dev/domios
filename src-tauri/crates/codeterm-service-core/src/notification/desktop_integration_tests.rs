use codeterm_notification_core::contract::NotifyLevel;

/// The rename is semantic, not cosmetic: the retired wire strings are rejected
/// at the transport boundary rather than quietly accepted.
#[test]
fn notify_level_wire_parse_rejects_the_retired_names() {
    use crate::notification::vet::parse_push_intent;

    for level in NotifyLevel::ALL {
        let intent = parse_push_intent(Some(level.as_str()), None)
            .unwrap_or_else(|e| panic!("{} must parse at the boundary: {e}", level.as_str()));
        assert_eq!(intent.level, Some(level));
    }
    for retired in ["milestone", "blocker", "progress", "Milestone"] {
        assert!(
            parse_push_intent(Some(retired), None).is_err(),
            "{retired} must be rejected on the wire, not passed through"
        );
    }
    assert_eq!(
        parse_push_intent(None, None)
            .expect("naming no level stays legal")
            .level,
        None,
        "omitting the level is still the fail-closed inbox path"
    );
}
