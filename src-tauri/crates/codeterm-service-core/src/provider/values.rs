pub fn list_agent_provider_values(
    registry: &crate::provider::ProviderRegistry,
) -> Vec<serde_json::Value> {
    let revision = registry.catalogue_revision();
    if let Some(values) = registry
        .catalogue_values_cache
        .lock()
        .get("providers", &revision)
    {
        return values;
    }
    let values = registry
        .describe_all()
        .into_iter()
        .filter_map(|descriptor| {
            let status = registry
                .cli_status(
                    &descriptor.id,
                    crate::agent::provider::current_platform_tag(),
                )
                .ok();
            let stub_replay = crate::agent::stub_launch::provider_stub_replay(&descriptor.id);
            let mut value = serde_json::to_value(descriptor).ok()?;
            value["cli_status"] = serde_json::to_value(status).ok()?;
            if let Some(stubbed) = stub_replay {
                value["stub_replay"] = stubbed.into();
            }
            Some(value)
        })
        .collect::<Vec<_>>();
    if registry.catalogue_revision() == revision {
        registry
            .catalogue_values_cache
            .lock()
            .insert("providers".into(), revision, values.clone());
    }
    values
}
