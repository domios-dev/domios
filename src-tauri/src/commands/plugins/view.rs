use crate::plugin::WebhookReceiverKind;
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
    view_call(&svc, &plugin_id, &method, args.unwrap_or(Value::Null)).await
}

/// Shared by the Tauri command and the web-mirror REST route.
pub(crate) async fn view_call(
    svc: &crate::service::ServiceState,
    plugin_id: &str,
    method: &str,
    args: Value,
) -> Result<Value, String> {
    let provider = svc
        .view_registry
        .get(plugin_id)
        .ok_or_else(|| format!("no view provider for plugin '{plugin_id}'"))?;
    if method == "webhookSettings" {
        let kind = provider
            .webhook_receiver_kind()
            .ok_or_else(|| "webhookSettings is unavailable for this view".to_string())?;
        return webhook_settings(svc, kind).await;
    }
    provider.call(method, args).await.map_err(|e| e.to_string())
}

async fn webhook_settings(
    svc: &crate::service::ServiceState,
    kind: WebhookReceiverKind,
) -> Result<Value, String> {
    let (secret_name, path) = match kind {
        WebhookReceiverKind::Pebble => (
            crate::api::inbound_webhook::PEBBLE_WEBHOOK_SECRET,
            crate::api::inbound_webhook::PEBBLE_WEBHOOK_PATH,
        ),
    };
    let store = { svc.secrets.read().await.clone() };
    let secret = store
        .get(secret_name)
        .await
        .map_err(|_| "webhook secret is unavailable".to_string())?;
    let token = secret.value.expose_secret().to_string();
    if token.is_empty() {
        return Err("webhook secret is empty".to_string());
    }
    let header_value = format!("Bearer {token}");
    let port = crate::config::load_config().api.port;
    let base_url = tokio::task::spawn_blocking(move || public_api_base_url(port))
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
    if let Some(host) = crate::api::network_routes::local_ip_address() {
        return format_base_url("http", &host, port);
    }
    crate::config::api_callback_url(&crate::config::load_config().api).0
}

#[cfg(test)]
mod tests {
    use super::format_base_url;

    #[test]
    fn webhook_setup_formats_ipv6_hosts_without_ambiguous_authority() {
        assert_eq!(
            format_base_url("http", "2001:db8::1", 7685),
            "http://[2001:db8::1]:7685"
        );
    }
}
