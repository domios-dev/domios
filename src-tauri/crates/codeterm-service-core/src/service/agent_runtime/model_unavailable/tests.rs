use super::*;

#[test]
fn model_unavailable_fallback_requires_a_fresh_implicit_selection_and_preacceptance_proof() {
    let explicit = Fallback::new(false);
    assert!(!explicit.eligible(true, true));
    let implicit = Fallback::new(true);
    assert!(!implicit.eligible(false, true));
    assert!(!implicit.eligible(true, false));
    assert!(implicit.eligible(true, true));
    implicit.clone().used.store(true, Ordering::Release);
    assert!(!implicit.eligible(true, true));
}
