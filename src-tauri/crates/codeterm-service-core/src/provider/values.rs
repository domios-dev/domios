pub fn list_agent_provider_values(
    registry: &crate::provider::ProviderRegistry,
) -> Vec<serde_json::Value> {
    let started = std::time::Instant::now();
    let revision = registry.catalogue_revision();
    let revision_us = started.elapsed().as_micros();
    let cache_started = std::time::Instant::now();
    let cached = registry
        .catalogue_values_cache
        .lock()
        .get("providers", &revision);
    if let Some(values) = cached.filter(|values| {
        super::catalogue_cache::misses_still_hold(
            super::catalogue_cache::unresolved_listing(values),
            super::catalogue_cache::binary_found,
        )
    }) {
        log::debug!(
            "[providers.timing] cache=hit revision_us={} cache_us={} total_us={}",
            revision_us,
            cache_started.elapsed().as_micros(),
            started.elapsed().as_micros()
        );
        return values;
    }
    let cache_us = cache_started.elapsed().as_micros();
    let descriptors_started = std::time::Instant::now();
    let descriptors = registry.describe_all();
    let descriptors_us = descriptors_started.elapsed().as_micros();
    let values_started = std::time::Instant::now();
    let values = descriptors
        .into_iter()
        .filter_map(|descriptor| {
            let cli_started = std::time::Instant::now();
            let status = registry
                .cli_status(
                    &descriptor.id,
                    crate::agent::provider::current_platform_tag(),
                )
                .ok();
            log::debug!(
                "[providers.cli] provider={} elapsed_us={}",
                descriptor.id,
                cli_started.elapsed().as_micros()
            );
            let stub_replay = crate::agent::stub_launch::provider_stub_replay(&descriptor.id);
            let mut value = serde_json::to_value(descriptor).ok()?;
            value["cli_status"] = serde_json::to_value(status).ok()?;
            if let Some(stubbed) = stub_replay {
                value["stub_replay"] = stubbed.into();
            }
            Some(value)
        })
        .collect::<Vec<_>>();
    let values_us = values_started.elapsed().as_micros();
    let publish_started = std::time::Instant::now();
    if registry.catalogue_revision() == revision {
        registry
            .catalogue_values_cache
            .lock()
            .insert("providers".into(), revision, values.clone());
    }
    log::debug!(
        "[providers.timing] cache=miss revision_us={} cache_us={} descriptors_us={} values_us={} publish_us={} total_us={}",
        revision_us, cache_us, descriptors_us, values_us, publish_started.elapsed().as_micros(), started.elapsed().as_micros()
    );
    values
}
