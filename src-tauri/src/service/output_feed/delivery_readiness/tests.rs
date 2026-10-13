use super::*;

fn state(modal_visible: bool) -> DeliveryReadiness {
    DeliveryReadiness {
        pty_id: "pty".into(),
        generation: Some(1),
        modal_visible,
    }
}

#[test]
fn output_burst_without_a_readiness_transition_never_wakes_owners() {
    let events = DeliveryReadinessEvents::default();
    assert!(!events.observe("tab", state(true)));
    for _ in 0..1000 {
        assert!(!events.observe("tab", state(true)));
    }
    assert!(events.observe("tab", state(false)));
    for _ in 0..1000 {
        assert!(!events.observe("tab", state(false)));
    }
}

#[test]
fn generation_and_tab_scope_are_preserved() {
    let events = DeliveryReadinessEvents::default();
    assert!(!events.observe("first", state(false)));
    assert!(!events.observe("second", state(false)));
    let mut replaced = state(false);
    replaced.generation = Some(2);
    assert!(events.observe("first", replaced.clone()));
    assert!(!events.observe("first", replaced));
    assert!(!events.observe("second", state(false)));
}

#[test]
fn a_prewrite_guard_cannot_consume_the_readiness_transition() {
    let events = DeliveryReadinessEvents::default();
    events.prime("tab", state(true));
    events.prime("tab", state(false));
    assert!(events.observe("tab", state(false)));
    assert!(!events.observe("tab", state(false)));
}
