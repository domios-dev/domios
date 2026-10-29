use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request as HttpRequest,
    routing::post,
    Router,
};
use codeterm_service_core::{
    multiplexer::{
        state::MultiplexerState,
        types::{SurfaceBacking, TabBacking},
    },
    service::test_fixture::ServiceFixture,
};
use serde_json::json;
use tower::ServiceExt;

struct Provider(String);
#[codeterm_plugin::async_trait]
impl ViewProvider for Provider {
    fn id(&self) -> &str {
        &self.0
    }
    async fn call(
        &self,
        _: &str,
        _: Value,
        _: &ViewCallContext,
    ) -> Result<Value, codeterm_plugin::view_host::ViewError> {
        unreachable!()
    }
}

#[tokio::test]
async fn plugin_run_view_route_requires_exact_private_document_and_owned_persisted_reference() {
    let root = tempfile::tempdir().unwrap();
    let plugin = format!("run-view-{}", uuid::Uuid::new_v4().simple());
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let tab = mux.create_tab(&workspace).unwrap();
    let surface = mux.get_tab_mut(&tab).unwrap();
    surface.parent_tab_id = Some(owner.clone());
    surface.backing = TabBacking::Surface(SurfaceBacking {
        plugin_id: plugin.clone(),
        surface_id: "studio".into(),
        instance_id: "instance".into(),
        owner_label: true,
        resume_url: None,
        resume_profile: None,
    });
    let fixture = ServiceFixture::new(mux);
    assert!(fixture
        .service
        .register_host_extension(Arc::new(codeterm_sentinel_host::PluginStepStoreRoot(
            root.path().to_path_buf()
        )))
        .is_ok());
    fixture
        .service
        .ext::<codeterm_plugin::surfaces::SurfaceRegistryHandle>()
        .register(
            &plugin,
            Arc::new(vec![serde_json::from_value(
                json!({"id":"studio","title":"Studio","renderer":"view"}),
            )
            .unwrap()]),
        );
    let registry = fixture.service.ext::<ViewRegistryHandle>();
    let provider: Arc<dyn ViewProvider> = Arc::new(Provider(plugin.clone()));
    registry.register(&plugin, provider.clone());
    let context = fixture
        .service
        .plugin_view_context(&plugin, Some("studio"), Some("instance"), Some(&tab))
        .unwrap();
    let frame =
        codeterm_plugin::media_delivery::create_frame(&provider, registry, context).unwrap();
    let ledger = codeterm_plugin::blueprint_step::OperationLedger::open(
        &root.path().join("blueprint-plugin-operations.sqlite"),
    )
    .unwrap();
    let scope = codeterm_plugin::blueprint_step::RunScope {
        workspace: workspace.clone(),
        team: None,
        run: "run".into(),
    };
    let view = ledger
        .run_view(&scope, &owner, "blueprint", "Fixture")
        .unwrap();
    let mut foreign = scope.clone();
    foreign.run = "foreign".into();
    foreign.workspace = "foreign".into();
    let foreign = ledger
        .run_view(&foreign, &owner, "blueprint", "Foreign")
        .unwrap();
    let router = Router::new()
        .route("/run", post(resolve))
        .with_state(crate::transport_tabs::fixture::app_state(&fixture))
        .layer(Extension(CallerIdentity::Local));
    let request = |reference: &str, lease: Option<&str>| {
        let mut request = HttpRequest::builder()
            .method("POST")
            .uri("/run")
            .header("content-type", "application/json");
        if let Some(lease) = lease {
            request = request
                .header("x-domios-plugin-view-frame", lease)
                .header("x-domios-plugin-media-frame", &frame.media_frame);
        }
        request
            .body(Body::from(
                json!({"pluginId":plugin,"reference":reference}).to_string(),
            ))
            .unwrap()
    };
    let response = router
        .clone()
        .oneshot(request(&view.reference, Some(&frame.view_lease)))
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"workspaceId":workspace,"target":{"blueprintId":"blueprint","runId":"run","teamId":null}})
    );
    for (reference, lease) in [
        (&view.reference, None),
        (&foreign.reference, Some(frame.view_lease.as_str())),
    ] {
        let response = router
            .clone()
            .oneshot(request(reference, lease))
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["ok"], false);
    }
    registry.register(&plugin, Arc::new(Provider(plugin.clone())));
    let response = router
        .oneshot(request(&view.reference, Some(&frame.view_lease)))
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["reason"],
        "stale"
    );
}
