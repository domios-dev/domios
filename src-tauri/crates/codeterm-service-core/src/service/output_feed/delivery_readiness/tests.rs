use super::*;

fn state(generation: u64) -> DeliveryReadiness {
    DeliveryReadiness {
        pty_id: "pty".into(),
        generation: Some(generation),
    }
}

#[test]
fn output_burst_without_a_generation_change_never_wakes_owners() {
    let events = DeliveryReadinessEvents::default();
    assert!(!events.observe("tab", state(1)));
    for _ in 0..1000 {
        assert!(!events.observe("tab", state(1)));
    }
    assert!(events.observe("tab", state(2)));
    for _ in 0..1000 {
        assert!(!events.observe("tab", state(2)));
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
