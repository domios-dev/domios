use crate::plugin::WebhookReceiverKind;
use serde_json::Value;

/// Shared by the Tauri command and the web-mirror REST route.
pub(crate) async fn glance_render(
    registry: &crate::glance::GlanceRegistry,
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

/// Shared by the Tauri command and the web-mirror REST route.
pub(crate) async fn view_call(
    svc: &crate::service::ServiceState,
    plugin_id: &str,
    method: &str,
    args: Value,
) -> Result<Value, String> {
    dispatch_view_call(
        svc.ext::<crate::view_host::ViewRegistryHandle>(),
        plugin_id,
        method,
        args,
        |kind| webhook_settings(svc, kind),
    )
    .await
}

async fn dispatch_view_call<F, Fut>(
    registry: &crate::view_host::ViewRegistry,
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
    svc: &crate::service::ServiceState,
    kind: WebhookReceiverKind,
) -> Result<Value, String> {
    let store = { svc.secrets.read().await.clone() };
    let port = crate::config::load_config().api.port;
    webhook_settings_with(store.as_ref(), kind, move || public_api_base_url(port)).await
}

async fn webhook_settings_with(
    store: &dyn crate::secrets::SecretStore,
    kind: WebhookReceiverKind,
    resolve_base_url: impl FnOnce() -> String + Send + 'static,
) -> Result<Value, String> {
    let (secret_name, path) = match kind {
        WebhookReceiverKind::Pebble => (
            crate::service::web_access::PEBBLE_WEBHOOK_SECRET,
            crate::service::web_access::PEBBLE_WEBHOOK_PATH,
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
    if let Some(host) = crate::service::web_access::local_ip_address() {
        return format_base_url("http", &host, port);
    }
    crate::config::api_callback_url(&crate::config::load_config().api).0
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use serde_json::Value;

    use super::{dispatch_view_call, format_base_url, webhook_settings_with};
    use crate::plugin::WebhookReceiverKind;
    use crate::secrets::mock::MockStore;
    use crate::secrets::{SaveRequest, SecretStore, SecretString};
    use crate::service::web_access::{PEBBLE_WEBHOOK_PATH, PEBBLE_WEBHOOK_SECRET};
    use crate::view_host::{ViewError, ViewProvider, ViewRegistry};

    #[tokio::test]
    async fn plugin_glance_without_provider_returns_http_200_null() {
        use axum::response::IntoResponse;

        let registry = crate::glance::GlanceRegistry::new();
        for action in [None, Some("refresh")] {
            let view = super::glance_render(&registry, "provider-less", action, Value::Null)
                .await
                .expect("optional capability is absent");
            assert!(view.is_none());
            let response = axum::Json(view).into_response();
            assert_eq!(response.status(), axum::http::StatusCode::OK);
            let body = axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap();
            assert_eq!(&body[..], b"null");
        }
    }

    struct StubGlance {
        fail: bool,
    }

    #[async_trait::async_trait]
    impl crate::glance::GlanceProvider for StubGlance {
        fn id(&self) -> &str {
            "fixture"
        }

        async fn render(
            &self,
            _ctx: Value,
        ) -> Result<codeterm_plugin::view::GlanceView, crate::glance::GlanceError> {
            if self.fail {
                return Err(crate::glance::GlanceError::Render("fixture-failure".into()));
            }
            Ok(codeterm_plugin::view::GlanceView {
                title: Some("fixture-title".into()),
                nodes: vec![],
            })
        }
    }

    #[tokio::test]
    async fn plugin_glance_preserves_provider_views_and_failures() {
        let registry = crate::glance::GlanceRegistry::new();
        registry.register("ready", Arc::new(StubGlance { fail: false }));
        registry.register("failed", Arc::new(StubGlance { fail: true }));
        for action in [None, Some("refresh")] {
            let view = super::glance_render(&registry, "ready", action, Value::Null)
                .await
                .unwrap()
                .expect("provider view remains present");
            assert_eq!(view.title.as_deref(), Some("fixture-title"));
            assert!(view.nodes.is_empty());
            let error = super::glance_render(&registry, "failed", action, Value::Null)
                .await
                .expect_err("provider failures remain errors");
            assert_eq!(error, "glance render failed: fixture-failure");
        }
    }

    const TOKEN: &str = "pebble-runtime-token";
    const BASE_URL: &str = "http://runtime-host:7685";

    struct StubView {
        id: &'static str,
        kind: Option<WebhookReceiverKind>,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl ViewProvider for StubView {
        fn id(&self) -> &str {
            self.id
        }

        fn webhook_receiver_kind(&self) -> Option<WebhookReceiverKind> {
            self.kind
        }

        async fn call(&self, method: &str, _args: Value) -> Result<Value, ViewError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({ "forwarded": method }))
        }
    }

    fn stub(id: &'static str, kind: Option<WebhookReceiverKind>) -> Arc<StubView> {
        Arc::new(StubView {
            id,
            kind,
            calls: AtomicUsize::new(0),
        })
    }

    async fn call(
        registry: &ViewRegistry,
        store: &MockStore,
        plugin_id: &str,
        method: &str,
        settings_requests: &AtomicUsize,
    ) -> Result<Value, String> {
        dispatch_view_call(registry, plugin_id, method, Value::Null, |kind| {
            settings_requests.fetch_add(1, Ordering::SeqCst);
            webhook_settings_with(store, kind, || BASE_URL.to_string())
        })
        .await
    }

    #[tokio::test]
    async fn webhook_settings_requires_capability() {
        let store = MockStore::new();
        store
            .save(SaveRequest {
                name: PEBBLE_WEBHOOK_SECRET.into(),
                value: SecretString::from(TOKEN),
                notes: None,
                folder: None,
                organization: None,
                collection: None,
                overwrite: false,
            })
            .await
            .expect("seed webhook secret");
        let plain = stub("plain", None);
        let pebble = stub("pebble", Some(WebhookReceiverKind::Pebble));
        let registry = ViewRegistry::new();
        registry.register("plain", plain.clone() as Arc<dyn ViewProvider>);
        registry.register("pebble", pebble.clone() as Arc<dyn ViewProvider>);
        let settings_requests = AtomicUsize::new(0);

        let refused = call(
            &registry,
            &store,
            "plain",
            "webhookSettings",
            &settings_requests,
        )
        .await
        .expect_err("a view without webhookReceiver must be refused");
        assert_eq!(settings_requests.load(Ordering::SeqCst), 0);
        assert_eq!(plain.calls.load(Ordering::SeqCst), 0);
        for leaked in [
            TOKEN,
            BASE_URL,
            PEBBLE_WEBHOOK_PATH,
            "Bearer",
            "Authorization",
        ] {
            assert!(!refused.contains(leaked), "refusal leaked {leaked}");
        }

        let forwarded = call(&registry, &store, "plain", "status", &settings_requests)
            .await
            .expect("ordinary bridge calls still reach viewCall");
        assert_eq!(forwarded, serde_json::json!({ "forwarded": "status" }));
        assert_eq!(settings_requests.load(Ordering::SeqCst), 0);

        let granted = call(
            &registry,
            &store,
            "pebble",
            "webhookSettings",
            &settings_requests,
        )
        .await
        .expect("the pebble-capable view receives its settings");
        assert_eq!(settings_requests.load(Ordering::SeqCst), 1);
        assert_eq!(pebble.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            granted,
            serde_json::json!({
                "url": format!("{BASE_URL}{PEBBLE_WEBHOOK_PATH}"),
                "headerName": "Authorization",
                "token": TOKEN,
                "headerValue": format!("Bearer {TOKEN}"),
                "secretName": PEBBLE_WEBHOOK_SECRET,
            })
        );
    }

    #[test]
    fn webhook_setup_formats_ipv6_hosts_without_ambiguous_authority() {
        assert_eq!(
            format_base_url("http", "2001:db8::1", 7685),
            "http://[2001:db8::1]:7685"
        );
    }
}
