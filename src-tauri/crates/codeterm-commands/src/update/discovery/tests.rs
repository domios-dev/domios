use super::*;
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
