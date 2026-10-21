use super::*;

pub fn update_inventory(ctx: PluginCtx, refresh: bool) -> Result<serde_json::Value, String> {
    let _guard = enter_ctx(ctx);
    let mut errors = std::collections::BTreeMap::new();
    if let Err(error) = ensure_default_plugin_channel() {
        errors.insert(DEFAULT_PLUGIN_CHANNEL_NAME.to_string(), error);
    }
    if refresh {
        for channel in channels_view() {
            if let Err(error) = refresh_channel(Some(&channel.name)) {
                errors.insert(channel.name, error);
            }
        }
    }
    if refresh && !errors.contains_key(SYSTEM_CHANNEL_NAME) {
        if let Err(error) = update_system_cached(None) {
            errors.insert(SYSTEM_CHANNEL_NAME.to_string(), error);
        }
    }
    let listed = list_installed()?;
    let items: Vec<serde_json::Value> = listed["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|plugin| plugin["update"]["availability"].clone())
        .collect();
    Ok(serde_json::json!({ "items": items, "errors": errors }))
}

pub fn sync_core_cached(ctx: PluginCtx) -> Result<(), String> {
    let _guard = enter_ctx(ctx);
    update_system_cached(None).map(|_| ())
}
