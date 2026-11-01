use super::*;
use codeterm_plugin::{
    chatbackend_provider::{ChatBackendError, ChatBackendProvider},
    view_host::ViewError,
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Backend(String);
impl ChatBackendProvider for Backend {
    fn id(&self) -> &str {
        &self.0
    }
    fn remote_task_capability(
        &self,
    ) -> Option<codeterm_plugin::remote_tasks::RemoteTaskCapability> {
        Some(serde_json::from_value(json!({"mode":"remote-mode","backend":"opaque","displayName":"Remote","availability":{"status":"available"},"environments":[],"followUp":false,"cancel":false,"applyDiff":false})).unwrap())
    }
    fn open_session(&self, _: Value) -> Result<String, ChatBackendError> {
        Ok("session".into())
    }
    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }
    fn poll(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<codeterm_service_core::agent::chat_types::StructuredChat, ChatBackendError> {
        Ok(Default::default())
    }
    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        Ok(())
    }
    fn list_models(&self) -> Result<Vec<codeterm_plugin::provider::ModelInfo>, ChatBackendError> {
        Ok(vec![])
    }
}
struct View {
    id: String,
    calls: AtomicUsize,
}
#[async_trait::async_trait]
impl ViewProvider for View {
    fn id(&self) -> &str {
        &self.id
    }
    async fn call(
        &self,
        method: &str,
        args: Value,
        context: &ViewCallContext,
    ) -> Result<Value, ViewError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"method":method,"args":args,"context":context}))
    }
}
fn fixture() -> (ServiceFixture, Arc<View>, String) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let tab = mux.create_tab(&workspace).unwrap();
    let id = format!("host-view-{}", uuid::Uuid::new_v4().simple());
    let remote = mux.get_tab_mut(&tab).unwrap();
    remote.backing = TabBacking::ChatBackend {
        plugin_id: id.clone(),
        session_id: "live-session".into(),
    };
    remote.restorable_launch.execution_mode = Some("remote-mode".into());
    let fixture = ServiceFixture::new(mux);
    let provider = Arc::new(View {
        id: id.clone(),
        calls: AtomicUsize::new(0),
    });
    fixture
        .service
        .ext::<ViewRegistryHandle>()
        .register(&id, provider.clone());
    fixture
        .service
        .chatbackend_registry
        .register(&id, Arc::new(Backend(id.clone())));
    (fixture, provider, tab)
}

#[tokio::test]
async fn remote_host_port_uses_only_live_tab_provider_session_and_three_allowed_methods() {
    let (fixture, provider, tab) = fixture();
    let svc = Arc::new(fixture.service.clone());
    for method in ["read", "refresh", "submit"] {
        let args = if method == "submit" {
            json!({"prompt":"opaque","branch":"main"})
        } else {
            Value::Null
        };
        let out = remote_task_call(&svc, &tab, provider.id(), "live-session", method, args)
            .await
            .unwrap();
        assert_eq!(out["args"]["sessionId"], "live-session");
        assert_eq!(out["context"]["origin"], "remote_task");
        assert_eq!(out["context"]["owner"], tab);
        assert!(out["context"]["frame"].is_null());
    }
    let before = provider.calls.load(Ordering::SeqCst);
    for (target, method, args) in [
        (&*tab, "other", Value::Null),
        (&*tab, "read", json!({"sessionId":"foreign"})),
        ("foreign", "read", Value::Null),
    ] {
        assert!(
            remote_task_call(&svc, target, provider.id(), "live-session", method, args)
                .await
                .is_err()
        );
    }
    assert!(remote_task_call(
        &svc,
        &tab,
        "foreign-plugin",
        "live-session",
        "read",
        Value::Null
    )
    .await
    .is_err());
    assert!(remote_task_call(
        &svc,
        &tab,
        provider.id(),
        "stale-session",
        "read",
        Value::Null
    )
    .await
    .is_err());
    svc.mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .restorable_launch
        .execution_mode = None;
    assert!(remote_task_call(
        &svc,
        &tab,
        provider.id(),
        "live-session",
        "read",
        Value::Null
    )
    .await
    .is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn recording_host_port_requires_declared_grant_and_calls_only_selection_without_owner_scope()
{
    let (fixture, provider, _) = fixture();
    let svc = Arc::new(fixture.service.clone());
    let args = json!({"inputs":[],"microphones":[]});
    assert!(recording_suggestion(&svc, provider.id(), args.clone())
        .await
        .is_err());
    let registry = svc.ext::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>();
    let declaration = || {
        serde_json::from_value(
            json!({"vendorId":1,"productId":2,"usagePage":3,"usage":4,"button":null}),
        )
        .unwrap()
    };
    registry.register(provider.id(), declaration(), false);
    assert!(recording_suggestion(&svc, provider.id(), args.clone())
        .await
        .is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    registry.register(provider.id(), declaration(), true);
    let out = recording_suggestion(&svc, provider.id(), args)
        .await
        .unwrap();
    assert_eq!(out["method"], "selectRecordingDevices");
    assert_eq!(out["context"]["origin"], "recording_setup");
    assert!(out["context"]["owner"].is_null());
    assert!(out["context"]["frame"].is_null());
    let before = provider.calls.load(Ordering::SeqCst);
    assert!(recording_suggestion(
        &svc,
        provider.id(),
        json!({"inputs":[],"microphones":[],"method":"submit"})
    )
    .await
    .is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), before);
}
