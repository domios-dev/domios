use super::{advance_version, current_version, invalidate, subscribe_changes};
use std::collections::HashMap;

#[test]
fn catalogue_versions_advance_per_provider_and_ignore_other_catalogues() {
    let mut versions = HashMap::new();

    assert_eq!(advance_version(&mut versions, "provider-a"), 1);
    assert_eq!(advance_version(&mut versions, "provider-b"), 1);
    assert_eq!(advance_version(&mut versions, "provider-a"), 2);
    assert_eq!(versions.get("provider-b"), Some(&1));
}

#[test]
fn catalogue_versions_saturate_without_wrapping() {
    let mut versions = HashMap::from([("provider-a".to_string(), u64::MAX)]);

    assert_eq!(advance_version(&mut versions, "provider-a"), u64::MAX);
}

#[test]
fn invalidation_advances_and_emits_the_provider_version() {
    let provider = format!("catalogue-event-{}", uuid::Uuid::new_v4());
    let mut changes = subscribe_changes();
    let prior_version = current_version(&provider);

    let change = invalidate(&provider);

    assert_eq!(change.provider, provider);
    assert_eq!(change.version, prior_version + 1);
    let observed =
        std::iter::from_fn(|| changes.try_recv().ok()).find(|event| event.provider == provider);
    assert_eq!(observed, Some(change));
}
