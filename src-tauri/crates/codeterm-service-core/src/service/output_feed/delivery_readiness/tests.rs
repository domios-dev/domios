use super::*;

fn state(generation: u64) -> DeliveryReadiness {
    DeliveryReadiness {
        pty_id: "pty".into(),
        generation: Some(generation),
    }
}

#[test]
fn generation_and_tab_scope_are_preserved() {
    let events = DeliveryReadinessEvents::default();
    assert!(!events.observe("first", state(1)));
    assert!(!events.observe("second", state(1)));
    assert!(events.observe("first", state(2)));
    assert!(!events.observe("first", state(2)));
    assert!(!events.observe("second", state(1)));
}

#[test]
fn a_prewrite_guard_cannot_consume_the_generation_transition() {
    let events = DeliveryReadinessEvents::default();
    events.prime("tab", state(1));
    events.prime("tab", state(2));
    assert!(events.observe("tab", state(2)));
    assert!(!events.observe("tab", state(2)));
}
