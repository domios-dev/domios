use super::effect_after_claim;
use std::cell::RefCell;

#[test]
fn blueprint_pipeline_effect_runs_after_one_successful_claim() {
    let events = RefCell::new(Vec::new());
    let result = effect_after_claim(
        || {
            events.borrow_mut().push("claim");
            true
        },
        || {
            events.borrow_mut().push("effect");
            7
        },
    );

    assert_eq!(result, Some(7));
    assert_eq!(events.into_inner(), ["claim", "effect"]);
}

#[test]
fn blueprint_pipeline_refused_claim_skips_the_effect() {
    let events = RefCell::new(Vec::new());
    let result = effect_after_claim(
        || {
            events.borrow_mut().push("claim");
            false
        },
        || {
            events.borrow_mut().push("effect");
        },
    );

    assert_eq!(result, None);
    assert_eq!(events.into_inner(), ["claim"]);
}
