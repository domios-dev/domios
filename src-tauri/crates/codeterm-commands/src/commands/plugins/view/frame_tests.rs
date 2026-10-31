use super::view_call;
use codeterm_plugin::{
    media_delivery as frames,
    view_host::{ViewCallContext, ViewError, ViewProvider, ViewRegistryHandle},
};
use codeterm_service_core::{
    multiplexer::{
        state::MultiplexerState,
        types::{SurfaceBacking, TabBacking},
    },
    service::test_fixture::ServiceFixture,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct ScopedView {
    id: String,
    calls: AtomicUsize,
    started: tokio::sync::Notify,
    finish: tokio::sync::Notify,
}
#[async_trait::async_trait]
impl ViewProvider for ScopedView {
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
        if args["result_owner"].as_str() != context.owner.as_deref() {
            return Err(ViewError::Call("foreign result".into()));
        }
        if method == "held" {
            self.started.notify_one();
            self.finish.notified().await;
        }
        Ok(json!({"context":context,"job":"retained-receipt"}))
    }
}
fn fixture() -> (
    ServiceFixture,
    Arc<ScopedView>,
    frames::FrameRegistration,
    String,
    String,
) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner = mux.create_tab(&workspace).unwrap();
    let tab = mux.create_tab(&workspace).unwrap();
    let provider = Arc::new(ScopedView {
        id: format!("scoped-{}", uuid::Uuid::new_v4().simple()),
        calls: AtomicUsize::new(0),
        started: Default::default(),
        finish: Default::default(),
    });
    let view = mux.get_tab_mut(&tab).unwrap();
    view.parent_tab_id = Some(owner.clone());
    view.backing = TabBacking::Surface(SurfaceBacking {
        plugin_id: provider.id.clone(),
        surface_id: "board".into(),
        instance_id: "one".into(),
        owner_label: true,
        resume_url: None,
        resume_profile: None,
    });
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .ext::<codeterm_plugin::surfaces::SurfaceRegistryHandle>()
        .register(
            provider.id(),
            Arc::new(vec![serde_json::from_value(
                json!({"id":"board","title":"Board","renderer":"view"}),
            )
            .unwrap()]),
        );
    let registry = fixture.service.ext::<ViewRegistryHandle>();
    let erased: Arc<dyn ViewProvider> = provider.clone();
    registry.register(provider.id(), erased.clone());
    let context = fixture
        .service
        .plugin_view_context(provider.id(), Some("board"), Some("one"), Some(&tab))
        .unwrap();
    let registration = frames::create_frame(&erased, registry, context).unwrap();
    (fixture, provider, registration, owner, tab)
}

#[tokio::test]
async fn view_call_rejects_forged_foreign_presentation_and_stale_leases_before_callback() {
    let (fixture, provider, registration, owner, _) = fixture();
    let svc = Arc::new(fixture.service.clone());
    for (plugin, lease) in [
        (provider.id(), "forged"),
        ("other-plugin", registration.view_lease.as_str()),
        (provider.id(), registration.media_frame.as_str()),
    ] {
        assert!(
            view_call(&svc, plugin, "read", json!({"result_owner":owner}), lease)
                .await
                .is_err()
        );
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    let result = view_call(
        &svc,
        provider.id(),
        "read",
        json!({"result_owner":owner,"owner":"forged","context":{"owner":"forged"}}),
        &registration.view_lease,
    )
    .await
    .unwrap();
    assert_eq!(result["context"]["owner"], owner);
    assert_eq!(result["context"]["actor"], json!({"kind":"local"}));
    assert!(!result.to_string().contains(&registration.view_lease));
    assert!(view_call(
        &svc,
        provider.id(),
        "read",
        json!({"result_owner":"other-owner","owner":owner}),
        &registration.view_lease
    )
    .await
    .is_err());
    frames::revoke_view_frame(provider.id(), &registration.view_lease);
    let before = provider.calls.load(Ordering::SeqCst);
    assert!(view_call(
        &svc,
        provider.id(),
        "read",
        json!({"result_owner":owner}),
        &registration.view_lease
    )
    .await
    .is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn view_call_revalidates_live_owner_before_callback_and_frame_after_await() {
    let (fixture, provider, registration, owner, tab) = fixture();
    let svc = Arc::new(fixture.service.clone());
    let workspace = svc.mux.read().list_workspaces()[0].id.clone();
    let other = svc.mux.write().create_tab(&workspace).unwrap();
    svc.mux.write().get_tab_mut(&tab).unwrap().parent_tab_id = Some(other);
    assert!(view_call(
        &svc,
        provider.id(),
        "read",
        json!({"result_owner":owner}),
        &registration.view_lease
    )
    .await
    .is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    svc.mux.write().get_tab_mut(&tab).unwrap().parent_tab_id = Some(owner.clone());
    let request = view_call(
        &svc,
        provider.id(),
        "held",
        json!({"result_owner":owner}),
        &registration.view_lease,
    );
    let close = async {
        provider.started.notified().await;
        frames::revoke_view_frame(provider.id(), &registration.view_lease);
        provider.finish.notify_one();
    };
    let (out, ()) = tokio::join!(request, close);
    assert!(out.unwrap_err().contains("plugin_view_frame_invalid"));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}
