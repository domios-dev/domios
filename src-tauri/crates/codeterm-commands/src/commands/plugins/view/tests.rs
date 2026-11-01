use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::Value;

use super::{dispatch_view_call, format_base_url, webhook_settings_with};
use codeterm_service_core::plugin::WebhookReceiverKind;
use codeterm_service_core::secrets::mock::MockStore;
use codeterm_service_core::secrets::{SaveRequest, SecretStore, SecretString};
use codeterm_service_core::service::web_access::{PEBBLE_WEBHOOK_PATH, PEBBLE_WEBHOOK_SECRET};
use codeterm_service_core::view_host::{ViewError, ViewProvider, ViewRegistry};

#[tokio::test]
async fn plugin_glance_without_provider_returns_http_200_null() {
    use axum::response::IntoResponse;

    let registry = codeterm_service_core::glance::GlanceRegistry::new();
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
impl codeterm_service_core::glance::GlanceProvider for StubGlance {
    fn id(&self) -> &str {
        "fixture"
    }

    async fn render(
        &self,
        _ctx: Value,
    ) -> Result<codeterm_plugin::view::GlanceView, codeterm_service_core::glance::GlanceError> {
        if self.fail {
            return Err(codeterm_service_core::glance::GlanceError::Render(
                "fixture-failure".into(),
            ));
        }
        Ok(codeterm_plugin::view::GlanceView {
            title: Some("fixture-title".into()),
            nodes: vec![],
        })
    }
}

#[tokio::test]
async fn plugin_glance_preserves_provider_views_and_failures() {
    let registry = codeterm_service_core::glance::GlanceRegistry::new();
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

    async fn call(
        &self,
        method: &str,
        _args: Value,
        _context: &codeterm_plugin::view_host::ViewCallContext,
    ) -> Result<Value, ViewError> {
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
    let provider = registry
        .get(plugin_id)
        .ok_or_else(|| format!("no view provider for plugin '{plugin_id}'"))?;
    let context = codeterm_plugin::view_host::ViewCallContext::panel(plugin_id);
    dispatch_view_call(provider, method, Value::Null, &context, |kind| {
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

#[tokio::test]
async fn plugin_bubble_absent_capability_returns_empty_result() {
    let registry = codeterm_service_core::bubble::BubbleRegistry::new();
    assert_eq!(
        super::bubble_poll(&registry, "providerless", Value::Null)
            .await
            .unwrap(),
        Value::Null
    );
}

struct StubBubble(bool);
#[async_trait::async_trait]
impl codeterm_service_core::bubble::BubbleProvider for StubBubble {
    fn id(&self) -> &str {
        "fixture"
    }
    async fn bubble(
        &self,
        _: Value,
    ) -> Result<
        Option<codeterm_service_core::bubble::BubbleData>,
        codeterm_service_core::bubble::BubbleError,
    > {
        if self.0 {
            Err(codeterm_service_core::bubble::BubbleError::Render(
                "fixture-failure".into(),
            ))
        } else {
            Ok(Some(codeterm_service_core::bubble::BubbleData {
                label: "fixture".into(),
                tone: Default::default(),
                icon: None,
                tooltip: None,
                action: None,
            }))
        }
    }
}

#[tokio::test]
async fn plugin_bubble_preserves_real_provider_results_and_errors() {
    let registry = codeterm_service_core::bubble::BubbleRegistry::new();
    registry.register("ready", Arc::new(StubBubble(false)));
    registry.register("failed", Arc::new(StubBubble(true)));
    assert_eq!(
        super::bubble_poll(&registry, "ready", Value::Null)
            .await
            .unwrap()["label"],
        "fixture"
    );
    assert_eq!(
        super::bubble_poll(&registry, "failed", Value::Null)
            .await
            .unwrap_err(),
        "bubble render failed: fixture-failure"
    );
}
