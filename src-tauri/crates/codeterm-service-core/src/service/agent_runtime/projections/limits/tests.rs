use super::*;
fn merge(
    previous: Option<Reading<Vec<RuntimeRateLimit>>>,
    incoming: Vec<RuntimeRateLimit>,
    at: u64,
) -> Reading<Vec<RuntimeRateLimit>> {
    super::merge(previous, incoming, at).unwrap()
}

fn window(name: &str, reset: Option<u64>) -> RuntimeRateLimit {
    RuntimeRateLimit {
        window: name.into(),
        used_percent: 42.0,
        resets_at: reset,
        window_minutes: Some(300),
    }
}

#[test]
fn partial_windows_and_credit_only_updates_preserve_known_resets() {
    let first = merge(
        None,
        vec![window("primary", Some(500)), window("secondary", Some(900))],
        100,
    );
    let partial = merge(Some(first), vec![window("primary", None)], 200);
    assert_eq!(partial.value[0].resets_at, Some(500));
    assert_eq!(partial.value[1].resets_at, Some(900));
    let credits = merge(Some(partial), vec![], 300);
    assert_eq!(credits.value.len(), 2);
    assert_eq!(credits.value[0].resets_at, Some(500));
}

#[test]
fn authoritative_reset_replaces_only_its_window_and_old_events_cannot_revert_it() {
    let first = merge(
        None,
        vec![
            window("primary", Some(900)),
            window("secondary", Some(1000)),
        ],
        100,
    );
    let newer = merge(Some(first), vec![window("primary", Some(600))], 300);
    let older = merge(Some(newer), vec![window("primary", Some(800))], 200);
    assert_eq!(older.value[0].resets_at, Some(600));
    assert_eq!(older.value[1].resets_at, Some(1000));
}

#[test]
fn expired_resets_clear_without_inventing_a_new_cycle() {
    let first = merge(
        None,
        vec![window("primary", Some(500)), window("secondary", None)],
        100,
    );
    let expired = merge(Some(first), vec![], 500);
    assert!(expired
        .value
        .iter()
        .all(|window| window.resets_at.is_none()));
    let first = merge(None, vec![window("primary", Some(900))], 100);
    let replaced = merge(Some(first), vec![window("primary", Some(200))], 300);
    let partial = merge(Some(replaced), vec![window("primary", None)], 400);
    assert_eq!(partial.value[0].resets_at, None);
}

#[test]
fn accumulated_windows_keep_the_runtime_resource_bound() {
    let first = merge(
        None,
        (0..4)
            .map(|index| window(&index.to_string(), Some(900)))
            .collect(),
        100,
    );
    assert!(super::merge(
        Some(first.clone()),
        vec![window("overflow", Some(1000))],
        200
    )
    .is_err());
    let replaced = merge(Some(first), vec![window("new", Some(1000))], 900);
    assert_eq!(replaced.value.len(), 1);
    assert_eq!(replaced.value[0].window, "new");
}
