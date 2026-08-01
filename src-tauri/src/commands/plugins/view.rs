use serde_json::Value;

pub(super) async fn plugin_glance_impl(
    svc: tauri::State<'_, crate::service::ServiceState>,
    plugin_id: String,
    action: Option<String>,
    ctx: Option<Value>,
) -> Result<Value, String> {
    glance_render(
        &svc.glance_registry,
        &plugin_id,
        action.as_deref(),
        ctx.unwrap_or(Value::Null),
    )
    .await
}

/// Shared by the Tauri command and the web-mirror REST route.
pub(crate) async fn glance_render(
    registry: &crate::glance::GlanceRegistry,
    plugin_id: &str,
    action: Option<&str>,
    ctx: Value,
) -> Result<Value, String> {
    let provider = registry
        .get(plugin_id)
        .ok_or_else(|| format!("no glance provider for plugin '{plugin_id}'"))?;
    let view = match action {
        Some(a) => provider.action(a, ctx).await,
        None => provider.render(ctx).await,
    }
    .map_err(|e| e.to_string())?;
    serde_json::to_value(view).map_err(|e| e.to_string())
}

/// Poll a single plugin's status bubble for a tab context. `null` = hide.
pub(super) async fn plugin_bubble_impl(
    svc: tauri::State<'_, crate::service::ServiceState>,
    plugin_id: String,
    ctx: Option<Value>,
) -> Result<Value, String> {
    bubble_poll(&svc.bubble_registry, &plugin_id, ctx.unwrap_or(Value::Null)).await
}

/// Shared by the Tauri command and the web-mirror REST route.
pub(crate) async fn bubble_poll(
    registry: &crate::bubble::BubbleRegistry,
    plugin_id: &str,
    ctx: Value,
) -> Result<Value, String> {
    let provider = registry
        .get(plugin_id)
        .ok_or_else(|| format!("no bubble provider for plugin '{plugin_id}'"))?;
    let pill = provider.bubble(ctx).await.map_err(|e| e.to_string())?;
    serde_json::to_value(pill).map_err(|e| e.to_string())
}

/// The iframe `ct.invoke(method, args)` bridge endpoint → the plugin's `viewCall`.
pub(super) async fn plugin_view_call_impl(
    svc: tauri::State<'_, crate::service::ServiceState>,
    plugin_id: String,
    method: String,
    args: Option<Value>,
) -> Result<Value, String> {
    view_call(
        &svc.view_registry,
        &plugin_id,
        &method,
        args.unwrap_or(Value::Null),
    )
    .await
}

/// Shared by the Tauri command and the web-mirror REST route.
pub(crate) async fn view_call(
    registry: &crate::view_host::ViewRegistry,
    plugin_id: &str,
    method: &str,
    args: Value,
) -> Result<Value, String> {
    let provider = registry
        .get(plugin_id)
        .ok_or_else(|| format!("no view provider for plugin '{plugin_id}'"))?;
    provider.call(method, args).await.map_err(|e| e.to_string())
}
