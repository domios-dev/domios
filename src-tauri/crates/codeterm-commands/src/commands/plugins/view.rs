use codeterm_service_core::plugin::WebhookReceiverKind;
use serde_json::Value;

/// Shared by the Tauri command and the web-mirror REST route.
pub async fn glance_render(
    registry: &codeterm_service_core::glance::GlanceRegistry,
    plugin_id: &str,
    action: Option<&str>,
    ctx: Value,
) -> Result<Option<codeterm_plugin::view::GlanceView>, String> {
    let Some(provider) = registry.get(plugin_id) else {
        return Ok(None);
    };
    let view = match action {
        Some(a) => provider.action(a, ctx).await,
        None => provider.render(ctx).await,
    }
    .map_err(|e| e.to_string())?;
    Ok(Some(view))
}

/// Shared by the Tauri command and the web-mirror REST route.
pub async fn bubble_poll(
    registry: &codeterm_service_core::bubble::BubbleRegistry,
    plugin_id: &str,
    ctx: Value,
) -> Result<Value, String> {
    let Some(provider) = registry.get(plugin_id) else {
        return Ok(Value::Null);
    };
    let pill = provider.bubble(ctx).await.map_err(|e| e.to_string())?;
    serde_json::to_value(pill).map_err(|e| e.to_string())
}

/// Shared by the Tauri command and the web-mirror REST route.
pub async fn view_call(
    svc: &codeterm_service_core::service::ServiceState,
    plugin_id: &str,
    method: &str,
    args: Value,
) -> Result<Value, String> {
    dispatch_view_call(
        svc.ext::<codeterm_service_core::view_host::ViewRegistryHandle>(),
        plugin_id,
        method,
        args,
        |kind| webhook_settings(svc, kind),
    )
    .await
}

async fn dispatch_view_call<F, Fut>(
    registry: &codeterm_service_core::view_host::ViewRegistry,
    plugin_id: &str,
    method: &str,
    args: Value,
    webhook_settings: F,
) -> Result<Value, String>
where
    F: FnOnce(WebhookReceiverKind) -> Fut,
    Fut: std::future::Future<Output = Result<Value, String>>,
{
    let provider = registry
        .get(plugin_id)
        .ok_or_else(|| format!("no view provider for plugin '{plugin_id}'"))?;
    if method == "webhookSettings" {
        let kind = provider
            .webhook_receiver_kind()
            .ok_or_else(|| "webhookSettings is unavailable for this view".to_string())?;
        return webhook_settings(kind).await;
    }
    provider.call(method, args).await.map_err(|e| e.to_string())
}

async fn webhook_settings(
    svc: &codeterm_service_core::service::ServiceState,
    kind: WebhookReceiverKind,
) -> Result<Value, String> {
    let store = { svc.secrets.read().await.clone() };
    let port = codeterm_service_core::config::load_config().api.port;
    webhook_settings_with(store.as_ref(), kind, move || public_api_base_url(port)).await
}

async fn webhook_settings_with(
    store: &dyn codeterm_service_core::secrets::SecretStore,
    kind: WebhookReceiverKind,
    resolve_base_url: impl FnOnce() -> String + Send + 'static,
) -> Result<Value, String> {
    let (secret_name, path) = match kind {
        WebhookReceiverKind::Pebble => (
            codeterm_service_core::service::web_access::PEBBLE_WEBHOOK_SECRET,
            codeterm_service_core::service::web_access::PEBBLE_WEBHOOK_PATH,
        ),
    };
    let secret = store
        .get(secret_name)
        .await
        .map_err(|_| "webhook secret is unavailable".to_string())?;
    let token = secret.value.expose_secret().to_string();
    if token.is_empty() {
        return Err("webhook secret is empty".to_string());
    }
    let header_value = format!("Bearer {token}");
    let base_url = tokio::task::spawn_blocking(resolve_base_url)
        .await
        .map_err(|error| format!("webhook URL lookup failed: {error}"))?;
    Ok(serde_json::json!({
        "url": format!("{base_url}{path}"),
        "headerName": "Authorization",
        "token": token,
        "headerValue": header_value,
        "secretName": secret_name,
    }))
}

fn format_base_url(scheme: &str, host: &str, port: u16) -> String {
    let authority = if host.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    format!("{scheme}://{authority}")
}

fn public_api_base_url(port: u16) -> String {
    if let Some(url) = codeterm_mesh::tailscale_serve_https_url(port) {
        return url;
    }
    if let Some(host) = codeterm_mesh::discovery::tailscale_self_dns_name() {
        return format_base_url("http", &host, port);
    }
    if let Some(host) = codeterm_mesh::discovery::find_tailscale_ip() {
        return format_base_url("http", &host, port);
    }
    if let Some(host) = codeterm_service_core::service::web_access::local_ip_address() {
        return format_base_url("http", &host, port);
    }
    codeterm_service_core::config::api_callback_url(
        &codeterm_service_core::config::load_config().api,
    )
    .0
}

#[cfg(test)]
mod tests;
