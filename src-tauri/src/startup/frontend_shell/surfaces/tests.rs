use super::*;
use std::cell::Cell;

#[test]
fn uncertain_visibility_or_failed_show_keeps_the_only_starting_surface() {
    let dismissed = Cell::new(0);
    let shown = Cell::new(0);
    let error = reveal_with(
        Err("visibility unavailable".into()),
        || {
            shown.set(shown.get() + 1);
            Ok(())
        },
        || dismissed.set(dismissed.get() + 1),
    );
    assert!(error.is_err());
    assert_eq!(shown.get(), 0);
    assert_eq!(dismissed.get(), 0);
    assert!(reveal_with(
        Ok(false),
        || Err("show unavailable".into()),
        || dismissed.set(dismissed.get() + 1),
    )
    .is_err());
    assert_eq!(dismissed.get(), 0);
}

#[test]
fn starting_surface_is_dismissed_only_for_a_proven_visible_main() {
    let dismissed = Cell::new(0);
    let shown = Cell::new(0);
    for visible in [true, false] {
        reveal_with(
            Ok(visible),
            || {
                shown.set(shown.get() + 1);
                Ok(())
            },
            || dismissed.set(dismissed.get() + 1),
        )
        .unwrap();
    }
    assert_eq!(shown.get(), 1);
    assert_eq!(dismissed.get(), 2);
}
