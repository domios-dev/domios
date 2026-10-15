use std::sync::Arc;

use codeterm_plugin::surfaces::{SurfaceDecl, SurfaceRestore};
use serde_json::Value;

use super::model::SurfaceError;
use super::runtime::{SurfaceCallCtx, SurfaceEffect, SurfaceEvents, SurfaceResume, SurfaceRuntime};
use crate::multiplexer::state::MultiplexerState;
use crate::service::browser::viewport::ViewportRequest;
use crate::service::message_gateway::DeliveryClass;
use crate::service::messenger::MessageEnvelope;
use crate::service::{ServiceContext, ServiceState};

const ANNOTATION_CONTEXT: &str = "surface-annotation";

/// The stream surface a tab id names, with the runtime its plugin declared and its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamBinding {
    pub tab_id: String,
    pub runtime: String,
    pub owner_tab_id: Option<String>,
    pub owner_name: String,
    /// The surface relaunches after a restart, so its resume point is applied and kept.
    pub relaunch: bool,
    pub resume: Option<SurfaceResume>,
}

pub(crate) fn stream_binding(
    mux: &MultiplexerState,
    raw_tab_id: &str,
    declared: impl Fn(&str) -> Option<Vec<SurfaceDecl>>,
) -> Result<StreamBinding, SurfaceError> {
    let tab_id = mux
        .resolve_tab_id(raw_tab_id)
        .ok_or_else(|| SurfaceError::TabNotFound(raw_tab_id.to_string()))?;
    let tab = mux
        .get_tab(&tab_id)
        .ok_or_else(|| SurfaceError::TabNotFound(raw_tab_id.to_string()))?;
    let surface = tab
        .surface()
        .ok_or_else(|| SurfaceError::NotAStreamSurface(tab_id.clone()))?;
    let (runtime, restore) = declared(&surface.plugin_id)
        .and_then(|surfaces| {
            surfaces
                .iter()
                .find(|decl| decl.id == surface.surface_id)
                .and_then(|decl| {
                    decl.stream_runtime()
                        .map(|runtime| (runtime.to_string(), decl.restore))
                })
        })
        .ok_or_else(|| SurfaceError::NotAStreamSurface(tab_id.clone()))?;
    let relaunch = restore == SurfaceRestore::Relaunch;
    let resume = relaunch
        .then(|| super::resume::backing_resume(surface))
        .flatten();
    let owner_tab_id = tab.parent_tab_id.clone();
    let owner_name = owner_tab_id
        .as_deref()
        .and_then(|owner| mux.get_tab(owner))
        .map(|owner| owner.title.clone())
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| "user".to_string());
    Ok(StreamBinding {
        tab_id,
        runtime,
        owner_tab_id,
        owner_name,
        relaunch,
        resume,
    })
}

/// Who drove a `/call`: the surface's own view, or an agent tab (`X-CodeTerm-Tab-Id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOrigin {
    View,
    Agent,
}

/// A refusal is the request's fault (4xx); a failure is the runtime's answer (`{ error }`).
#[derive(Debug)]
pub enum SurfaceCallFailure {
    Refused(SurfaceError),
    Failed(String),
}

impl ServiceContext {
    pub(crate) fn surface_runtime(
        &self,
        raw_tab_id: &str,
    ) -> Result<(Arc<dyn SurfaceRuntime>, StreamBinding), SurfaceError> {
        let binding = {
            let mux = self.mux.read();
            stream_binding(&mux, raw_tab_id, |plugin_id| {
                self.ext::<codeterm_plugin::surfaces::SurfaceRegistryHandle>()
                    .get(plugin_id)
                    .map(|declared| declared.to_vec())
            })?
        };
        let (runtime, created) = self
            .ext::<Arc<crate::service::surface::SurfaceRuntimeRegistry>>()
            .get_or_create(
                &binding.tab_id,
                &binding.runtime,
                &binding.owner_name,
                &binding.owner_tab_id.clone().unwrap_or_else(|| {
                    let mux = self.mux.read();
                    format!(
                        "workspace_{}",
                        mux.find_workspace_for_tab(&binding.tab_id)
                            .unwrap_or_default()
                    )
                }),
                binding.resume.clone(),
            )?;
        runtime.set_owner_name(&binding.owner_name);
        if created && binding.relaunch {
            if let Some(updates) = runtime.resume_updates() {
                self.follow_surface_resume(binding.tab_id.clone(), updates);
            }
        }
        Ok((runtime, binding))
    }

    pub(crate) fn surface_call_ctx(&self, binding: &StreamBinding) -> SurfaceCallCtx {
        SurfaceCallCtx {
            owner_tab_id: binding.owner_tab_id.clone(),
            data_dir: self.agent_pad_data_dir.clone(),
            actor: None,
        }
    }

    /// Owner, Team and workspace closes remove surface tabs inside the mux; their runtimes go here.
    pub fn dispose_closed_surface_runtimes(&self) {
        let closed = self
            .ext::<Arc<crate::service::surface::SurfaceRuntimeRegistry>>()
            .take_closed(|tab_id| self.mux.read().get_tab(tab_id).is_some());
        for runtime in closed {
            crate::async_runtime::spawn(async move { runtime.dispose().await });
        }
    }

    /// Blocking: a guarded system notice, so a bare shell owner is refused rather than typed into.
    pub(super) fn deliver_surface_annotation(
        &self,
        owner_tab_id: &str,
        markdown: &str,
    ) -> Result<(), String> {
        self.gateway_deliver(
            owner_tab_id,
            MessageEnvelope::codeterm_reminder(ANNOTATION_CONTEXT),
            markdown,
            DeliveryClass::SystemNotice,
        )
        .map(|_| ())
    }
}

pub async fn open_surface_stream(
    svc: &ServiceState,
    tab_id: &str,
    viewport: Option<ViewportRequest>,
) -> Result<SurfaceEvents, SurfaceError> {
    let (runtime, _) = svc.surface_runtime(tab_id)?;
    Ok(runtime.connect(viewport).await)
}

pub async fn call_surface(
    svc: &ServiceState,
    tab_id: &str,
    method: &str,
    args: Value,
    origin: CallOrigin,
) -> Result<Value, SurfaceCallFailure> {
    let (runtime, binding) = svc
        .surface_runtime(tab_id)
        .map_err(SurfaceCallFailure::Refused)?;
    if origin == CallOrigin::Agent && super::is_agent_method(method) {
        svc.note_surface_activity(&binding.tab_id, method, None);
    }
    let ctx = svc.surface_call_ctx(&binding);
    let outcome = runtime
        .call(&ctx, method, args)
        .await
        .map_err(SurfaceCallFailure::Failed)?;
    let mut result = outcome.result;
    for effect in outcome.effects {
        match effect {
            SurfaceEffect::DeliverAnnotation { id, markdown } => {
                let delivered = match binding.owner_tab_id.clone() {
                    Some(owner) => deliver_annotation(svc, owner, markdown).await,
                    None => Err("the surface has no owner".to_string()),
                };
                match &delivered {
                    Ok(()) => runtime.annotation_delivered(&id),
                    Err(error) => log::warn!("[surface] annotation {id} kept for inbox: {error}"),
                }
                if let Some(object) = result.as_object_mut() {
                    object.insert("delivered".into(), Value::Bool(delivered.is_ok()));
                }
            }
        }
    }
    Ok(result)
}

async fn deliver_annotation(
    svc: &ServiceState,
    owner: String,
    markdown: String,
) -> Result<(), String> {
    let service = svc.clone();
    tokio::task::spawn_blocking(move || service.deliver_surface_annotation(&owner, &markdown))
        .await
        .map_err(|error| format!("annotation delivery task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use codeterm_plugin::surfaces::SurfaceRenderer;

    use super::*;
    use crate::multiplexer::types::{SurfaceBacking, TabBacking};

    fn decl(id: &str, runtime: Option<&str>) -> SurfaceDecl {
        decl_restoring(id, runtime, SurfaceRestore::Placeholder)
    }

    fn decl_restoring(id: &str, runtime: Option<&str>, restore: SurfaceRestore) -> SurfaceDecl {
        SurfaceDecl {
            id: id.into(),
            title: "Browser".into(),
            renderer: if runtime.is_some() {
                SurfaceRenderer::Stream
            } else {
                SurfaceRenderer::View
            },
            agent_bound: true,
            restore,
            runtime: runtime.map(str::to_string),
            chrome: None,
            icon: None,
        }
    }

    fn mux_with_surface(surface_id: &str) -> (MultiplexerState, String, String) {
        let (mut mux, owner_id) = MultiplexerState::new();
        let workspace_id = mux.list_workspaces()[0].id.clone();
        mux.rename_tab(&owner_id, "Jev").unwrap();
        let tab_id = mux.create_tab(&workspace_id).unwrap();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.backing = TabBacking::Surface(SurfaceBacking {
            plugin_id: "web".into(),
            surface_id: surface_id.into(),
            instance_id: "i".into(),
            owner_label: true,
            resume_url: None,
            resume_profile: None,
        });
        tab.parent_tab_id = Some(owner_id.clone());
        (mux, owner_id, tab_id)
    }

    #[test]
    fn surface_stream_binding_reads_runtime_and_owner_from_the_declaration() {
        let (mux, owner_id, tab_id) = mux_with_surface("browser");
        let binding = stream_binding(&mux, &tab_id, |plugin| {
            (plugin == "web").then(|| vec![decl("browser", Some("browser"))])
        })
        .unwrap();
        assert_eq!(binding.runtime, "browser");
        assert_eq!(binding.owner_tab_id.as_deref(), Some(owner_id.as_str()));
        assert_eq!(binding.owner_name, "Jev");
    }

    fn remember(mux: &mut MultiplexerState, tab_id: &str) {
        let TabBacking::Surface(backing) = &mut mux.get_tab_mut(tab_id).unwrap().backing else {
            panic!("expected a surface");
        };
        backing.resume_url = Some("https://a.dev/inbox".into());
        backing.resume_profile = Some("work".into());
    }

    #[test]
    fn surface_stream_binding_resumes_only_relaunching_surfaces() {
        let (mut mux, _, tab_id) = mux_with_surface("browser");
        remember(&mut mux, &tab_id);
        let with = |restore| {
            stream_binding(&mux, &tab_id, move |_| {
                Some(vec![decl_restoring("browser", Some("browser"), restore)])
            })
            .unwrap()
        };
        let relaunch = with(SurfaceRestore::Relaunch);
        assert!(relaunch.relaunch);
        assert_eq!(
            relaunch.resume,
            Some(SurfaceResume {
                url: Some("https://a.dev/inbox".into()),
                profile: Some("work".into()),
            })
        );
        for restore in [SurfaceRestore::Placeholder, SurfaceRestore::Discard] {
            let binding = with(restore);
            assert!(!binding.relaunch);
            assert_eq!(binding.resume, None);
        }
    }

    #[test]
    fn surface_stream_binding_has_no_resume_for_a_fresh_surface() {
        let (mux, _, tab_id) = mux_with_surface("browser");
        let binding = stream_binding(&mux, &tab_id, |_| {
            Some(vec![decl_restoring(
                "browser",
                Some("browser"),
                SurfaceRestore::Relaunch,
            )])
        })
        .unwrap();
        assert!(binding.relaunch);
        assert_eq!(binding.resume, None);
    }

    #[test]
    fn surface_stream_binding_refuses_view_surfaces_and_plain_tabs() {
        let (mux, owner_id, tab_id) = mux_with_surface("board");
        let view = |_: &str| Some(vec![decl("board", None)]);
        assert_eq!(
            stream_binding(&mux, &tab_id, view).unwrap_err().code(),
            "not_a_stream_surface"
        );
        assert_eq!(
            stream_binding(&mux, &owner_id, view).unwrap_err().code(),
            "not_a_stream_surface"
        );
        assert_eq!(
            stream_binding(&mux, "missing", view).unwrap_err().code(),
            "tab_not_found"
        );
        assert_eq!(
            stream_binding(&mux, &tab_id, |_| None).unwrap_err().code(),
            "not_a_stream_surface"
        );
    }
}
