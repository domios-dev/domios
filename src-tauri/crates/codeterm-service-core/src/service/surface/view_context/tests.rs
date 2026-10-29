use super::*;
use crate::multiplexer::types::{SurfaceBacking, TabBacking};
use crate::service::test_fixture::ServiceFixture;

fn fixture() -> (ServiceFixture, String, String, String) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&owner).unwrap().team_id = Some("team-one".into());
    let tab = mux.create_tab(&workspace).unwrap();
    let view = mux.get_tab_mut(&tab).unwrap();
    view.parent_tab_id = Some(owner.clone());
    view.backing = TabBacking::Surface(SurfaceBacking {
        plugin_id: "studio".into(),
        surface_id: "board".into(),
        instance_id: "instance-one".into(),
        owner_label: true,
        resume_url: None,
        resume_profile: None,
    });
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .ext::<codeterm_plugin::surfaces::SurfaceRegistryHandle>()
        .register(
            "studio",
            std::sync::Arc::new(vec![serde_json::from_value(
                serde_json::json!({"id":"board","title":"Board","renderer":"view"}),
            )
            .unwrap()]),
        );
    (fixture, workspace, owner, tab)
}

#[test]
fn trusted_view_tuple_projects_exact_owner_workspace_team_and_local_actor() {
    let (fixture, workspace, owner, tab) = fixture();
    let context = fixture
        .service
        .plugin_view_context("studio", Some("board"), Some("instance-one"), Some(&tab))
        .unwrap();
    assert_eq!(context.owner.as_deref(), Some(owner.as_str()));
    assert_eq!(context.workspace.as_deref(), Some(workspace.as_str()));
    assert_eq!(context.team.as_deref(), Some("team-one"));
    assert_eq!(context.actor, codeterm_plugin::view_host::ViewActor::Local);
    fixture
        .service
        .validate_plugin_view_context(&context)
        .unwrap();
    let panel = fixture
        .service
        .plugin_view_context("studio", None, None, None)
        .unwrap();
    assert!(panel.owner_scope().is_none());
    assert!(panel.workspace.is_none());
}

#[test]
fn forged_partial_foreign_and_stale_view_tuples_never_resolve() {
    let (fixture, _, _, tab) = fixture();
    for (plugin, surface, instance, target) in [
        ("studio", Some("board"), None, Some(tab.as_str())),
        ("studio", None, None, Some(tab.as_str())),
        ("studio", Some("board"), Some("old"), Some(tab.as_str())),
        (
            "other-plugin",
            Some("board"),
            Some("instance-one"),
            Some(tab.as_str()),
        ),
        (
            "studio",
            Some("other"),
            Some("instance-one"),
            Some(tab.as_str()),
        ),
        (
            "studio",
            Some("board"),
            Some("instance-one"),
            Some("foreign"),
        ),
    ] {
        assert!(fixture
            .service
            .plugin_view_context(plugin, surface, instance, target)
            .is_err());
    }
}

#[test]
fn registered_owner_projection_refuses_reparent_team_change_and_close() {
    let (fixture, workspace, owner, tab) = fixture();
    let context = fixture
        .service
        .plugin_view_context("studio", Some("board"), Some("instance-one"), Some(&tab))
        .unwrap();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&owner)
        .unwrap()
        .team_id = Some("team-two".into());
    assert!(fixture
        .service
        .validate_plugin_view_context(&context)
        .is_err());
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&owner)
        .unwrap()
        .team_id = Some("team-one".into());
    let other = fixture.service.mux.write().create_tab(&workspace).unwrap();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .parent_tab_id = Some(other);
    assert!(fixture
        .service
        .validate_plugin_view_context(&context)
        .is_err());
    fixture.service.mux.write().remove_tab(&tab).unwrap();
    assert!(fixture
        .service
        .validate_plugin_view_context(&context)
        .is_err());
}

struct View;
#[async_trait::async_trait]
impl codeterm_plugin::view_host::ViewProvider for View {
    fn id(&self) -> &str {
        "studio"
    }
    async fn call(
        &self,
        _method: &str,
        _args: serde_json::Value,
        _context: &ViewCallContext,
    ) -> Result<serde_json::Value, codeterm_plugin::view_host::ViewError> {
        Ok(serde_json::Value::Null)
    }
}

#[test]
fn monitor_invalidation_derives_only_live_registered_instances_in_the_host_owner_scope() {
    use codeterm_plugin::{
        media_delivery as frames,
        surfaces::SurfaceRegistryHandle,
        view_host::{ViewProvider, ViewRegistryHandle},
    };
    use std::sync::Arc;
    let (fixture, _, _, tab) = fixture();
    let registry = fixture.service.ext::<ViewRegistryHandle>();
    let provider: Arc<dyn ViewProvider> = Arc::new(View);
    registry.register("studio", provider.clone());
    let context = fixture
        .service
        .plugin_view_context("studio", Some("board"), Some("instance-one"), Some(&tab))
        .unwrap();
    let scope = context.owner_scope().unwrap();
    let registration = frames::create_frame(&provider, registry, context).unwrap();
    fixture.service.ext::<SurfaceRegistryHandle>().register("studio",Arc::new(vec![serde_json::from_value(serde_json::json!({"id":"board","title":"Board","renderer":"view","stateInvalidation":true})).unwrap()]));
    let mut wrong = scope.clone();
    wrong.owner = "foreign".into();
    assert_eq!(
        fixture
            .service
            .invalidate_plugin_owner_views(&wrong)
            .unwrap(),
        0
    );
    wrong = scope.clone();
    wrong.workspace = "foreign".into();
    assert_eq!(
        fixture
            .service
            .invalidate_plugin_owner_views(&wrong)
            .unwrap(),
        0
    );
    wrong = scope.clone();
    wrong.team = None;
    assert_eq!(
        fixture
            .service
            .invalidate_plugin_owner_views(&wrong)
            .unwrap(),
        0
    );
    assert_eq!(
        fixture
            .service
            .invalidate_plugin_owner_views(&scope)
            .unwrap(),
        1
    );
    let captured = frames::frame_context("studio", &registration.media_frame).unwrap();
    let ran = std::cell::Cell::new(false);
    assert!(fixture
        .service
        .with_validated_plugin_view_context("foreign", &captured, || {
            ran.set(true);
            Ok(())
        })
        .is_err());
    assert!(!ran.get());
    assert_eq!(
        fixture
            .service
            .with_validated_plugin_view_context("studio", &captured, || Ok("opaque-input"))
            .unwrap(),
        "opaque-input"
    );
    assert!(fixture
        .service
        .with_validated_plugin_view_context("studio", &captured, || {
            frames::revoke_view_frame("studio", &registration.view_lease);
            Ok("discarded-grant")
        })
        .is_err());
    frames::revoke_view_frame("studio", &registration.view_lease);
    assert_eq!(
        fixture
            .service
            .invalidate_plugin_owner_views(&scope)
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn retired_document_continuation_cannot_admit_new_work_after_a_fresh_registration() {
    use codeterm_plugin::{
        media_delivery as frames,
        view_host::{with_view_call_context, ViewProvider, ViewRegistryHandle},
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let (fixture, _, _, tab) = fixture();
    let registry = fixture.service.ext::<ViewRegistryHandle>();
    let provider: Arc<dyn ViewProvider> = Arc::new(View);
    registry.register("studio", provider.clone());
    let scope = fixture
        .service
        .plugin_view_context("studio", Some("board"), Some("instance-one"), Some(&tab))
        .unwrap();
    let registration = frames::create_frame(&provider, registry, scope.clone()).unwrap();
    let captured = frames::frame_context("studio", &registration.media_frame).unwrap();
    let (started, observe_started) = tokio::sync::oneshot::channel();
    let (resume, continuation) = std::sync::mpsc::channel();
    let effects = Arc::new(AtomicUsize::new(0));
    let receipts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let worker_receipts = receipts.clone();
    let worker_effects = effects.clone();
    let svc = fixture.service.clone();
    let worker = tokio::task::spawn_blocking(move || {
        with_view_call_context("studio", &captured, || {
            worker_receipts
                .lock()
                .unwrap()
                .push("already-committed-receipt");
            started.send(()).unwrap();
            continuation.recv().unwrap();
            svc.with_validated_plugin_view_context("studio", &captured, || {
                worker_effects.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .map_err(codeterm_plugin::view_host::ViewError::Call)
        })
    });
    observe_started.await.unwrap();
    frames::revoke_frame("studio", &registration.media_frame);
    assert!(frames::view_frame("studio", &registration.view_lease).is_err());
    let fresh =
        frames::reregister_frame(&provider, registry, scope, &registration.media_frame).unwrap();
    resume.send(()).unwrap();
    assert!(worker.await.unwrap().is_err());
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert_eq!(*receipts.lock().unwrap(), vec!["already-committed-receipt"]);
    fixture
        .service
        .with_validated_plugin_view_context("studio", &fresh.context, || Ok(()))
        .unwrap();
    frames::revoke_frame("studio", &registration.media_frame);
}
