use super::*;

fn capture(fx: &Fixture, session: &str) -> FirstMaterialization {
    let path = fx.session_path(session);
    let (window, witness) = jsonl_boundary(Some(path.clone()), &fx.source, session);
    assert!(window.is_none());
    FirstMaterialization::new(fx.anchor_now(session), path, witness.unwrap())
}

#[test]
fn first_materialization_confirms_only_exact_own_session_input() {
    let fx = Fixture::new();
    let pending = capture(&fx, "own");
    assert!(pending.resolve(&fx.source).is_none());
    fx.append_user_turn("sibling", "opaque-input");
    assert!(pending.resolve(&fx.source).is_none());
    fx.append_user_turn("own", "other-input");
    let first_len = std::fs::metadata(fx.session_path("own")).unwrap().len();
    let anchor = pending.resolve(&fx.source).unwrap();
    let mut cursor = ExactTurnCursor::for_anchor(Some(&anchor));
    assert!(!contains_exact_user_turn_since(
        &fx.registry,
        &anchor,
        "opaque-input",
        &mut cursor
    ));
    fx.append_user_turn("own", "opaque-input");
    assert!(contains_exact_user_turn_since(
        &fx.registry,
        &anchor,
        "opaque-input",
        &mut cursor
    ));
    assert_eq!(*fx.parse_offsets.lock().unwrap(), vec![0, first_len]);
}

#[test]
fn first_materialization_witness_does_not_authorize_replacement_or_rebinding() {
    let fx = Fixture::new();
    let pending = capture(&fx, "own");
    fx.append_user_turn("own", "opaque-input");
    assert!(pending.resolve(&fx.source).is_some());
    std::fs::remove_file(fx.session_path("own")).unwrap();
    assert!(pending.resolve(&fx.source).is_none());
    fx.append_user_turn("own", "opaque-input");
    assert!(pending.resolve(&fx.source).is_none());
    let _ = fx.source.capture("next", &fx.session_path("next"));
    assert!(pending.resolve(&fx.source).is_none());
}

#[test]
fn first_materialization_rejects_a_foreign_absence_witness() {
    let fx = Fixture::new();
    let captured = capture(&fx, "own");
    let foreign = FirstMaterialization::new(
        captured.watch_anchor.clone(),
        fx.session_path("own"),
        "foreign".into(),
    );
    fx.append_user_turn("own", "opaque-input");
    assert!(foreign.resolve(&fx.source).is_none());
    assert!(captured.resolve(&fx.source).is_some());
}
