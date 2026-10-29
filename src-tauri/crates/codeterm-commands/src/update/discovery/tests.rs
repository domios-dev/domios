use super::*;

#[test]
fn cached_checks_only_enable_an_app_update_when_a_newer_payload_exists() {
    let current = AvailabilityItem::release("app", UpdateKind::App, "1.0.0", "1.0.0", true);
    assert_eq!(cached_app_availability(current).reason, "cached_check");
    let incompatible = AvailabilityItem::release("app", UpdateKind::App, "1.0.0", "2.0.0", false);
    assert_eq!(cached_app_availability(incompatible).reason, "cached_check");
    let newer = AvailabilityItem::release("app", UpdateKind::App, "1.0.0", "2.0.0", true);
    assert_eq!(cached_app_availability(newer).reason, "cached_manifest");
}
#[test]
fn action_completion_replaces_installed_states_and_preserves_other_updates() {
    let mut snapshot = AvailabilitySnapshot::default();
    snapshot.items.push(AvailabilityItem::release(
        "app",
        UpdateKind::App,
        "1.0.0",
        "2.0.0",
        true,
    ));
    replace_plugins(
        &mut snapshot,
        vec![AvailabilityItem::release(
            "lark",
            UpdateKind::Plugin,
            "0.1.2",
            "0.1.3",
            true,
        )],
    );
    assert_eq!(
        snapshot.items[1].state,
        codeterm_plugins::AvailabilityState::UpdateAvailable
    );
    replace_plugins(
        &mut snapshot,
        vec![AvailabilityItem::release(
            "lark",
            UpdateKind::Plugin,
            "0.1.3",
            "0.1.3",
            true,
        )],
    );
    assert_eq!(
        snapshot.items[1].state,
        codeterm_plugins::AvailabilityState::UpToDate
    );
    assert_eq!(
        snapshot.items[0].state,
        codeterm_plugins::AvailabilityState::UpdateAvailable
    );
    replace_plugins(&mut snapshot, vec![]);
    assert_eq!(snapshot.items.len(), 1);
}

#[test]
fn offline_attempt_is_cached_and_keeps_the_installed_app_visible() {
    let mut snapshot = AvailabilitySnapshot::default();
    record_failure(&mut snapshot, "offline", 80_000);
    assert!(cache::fresh(snapshot.checked_at_ms, 100_000));
    assert_eq!(snapshot.errors["discovery"], "offline");
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].kind, UpdateKind::App);
    assert_eq!(snapshot.items[0].latest, None);
    ensure_app(&mut snapshot);
    assert_eq!(snapshot.items.len(), 1);
}
