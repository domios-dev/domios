use super::{
    advance_version, current_version, invalidate, registry, subscribe_changes, ModelInfo,
    ProviderModels,
};
use std::collections::HashMap;
use std::time::SystemTime;

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
fn identical_rediscovery_does_not_advance_or_emit_catalogue_version() {
    let provider = format!("catalogue-event-{}", uuid::Uuid::new_v4());
    let mut changes = subscribe_changes();
    let prior_version = current_version(&provider);

    registry().write().insert(
        provider.clone(),
        ProviderModels::live(
            vec![model("model-a")],
            Some("model-a".into()),
            SystemTime::now(),
        ),
    );
    invalidate(&provider);
    let first_version = current_version(&provider);
    assert_eq!(first_version, prior_version + 1);
    assert_eq!(
        std::iter::from_fn(|| changes.try_recv().ok())
            .find(|event| event.provider == provider)
            .map(|event| event.version),
        Some(first_version)
    );

    registry().write().insert(
        provider.clone(),
        ProviderModels::live(
            vec![model("model-a")],
            Some("model-a".into()),
            SystemTime::now(),
        ),
    );
    invalidate(&provider);

    assert_eq!(current_version(&provider), first_version);
    assert!(std::iter::from_fn(|| changes.try_recv().ok()).all(|event| event.provider != provider));
    registry().write().remove(&provider);
}

#[test]
fn changed_catalogue_publishes_exactly_one_new_version() {
    let provider = format!("catalogue-change-{}", uuid::Uuid::new_v4());
    let mut changes = subscribe_changes();

    registry().write().insert(
        provider.clone(),
        ProviderModels::live(
            vec![model("model-a")],
            Some("model-a".into()),
            SystemTime::now(),
        ),
    );
    invalidate(&provider);
    let first_version = current_version(&provider);
    let _ = std::iter::from_fn(|| changes.try_recv().ok()).find(|event| event.provider == provider);

    registry().write().insert(
        provider.clone(),
        ProviderModels::live(
            vec![model("model-b")],
            Some("model-b".into()),
            SystemTime::now(),
        ),
    );
    invalidate(&provider);
    let changed_version = current_version(&provider);
    assert_eq!(changed_version, first_version + 1);
    let changed_events = std::iter::from_fn(|| changes.try_recv().ok())
        .filter(|event| event.provider == provider)
        .collect::<Vec<_>>();
    assert_eq!(changed_events.len(), 1);
    assert_eq!(changed_events[0].version, changed_version);

    registry().write().insert(
        provider.clone(),
        ProviderModels::live(
            vec![model("model-b")],
            Some("model-b".into()),
            SystemTime::now(),
        ),
    );
    invalidate(&provider);
    assert_eq!(current_version(&provider), changed_version);
    assert!(std::iter::from_fn(|| changes.try_recv().ok()).all(|event| event.provider != provider));
    registry().write().remove(&provider);
}

fn model(model_id: &str) -> ModelInfo {
    ModelInfo {
        id: model_id.into(),
        display_name: model_id.into(),
        ..Default::default()
    }
}
