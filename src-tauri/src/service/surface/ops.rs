use serde::Serialize;

use super::model::{
    choose_owner, owner_label, parse_surface_target, resolve_surface_decl, surface_placement,
    surface_renderable, surface_tab_row, DeclaredSurfaceRow, PluginSurfaceFacts, SurfaceError,
    SurfaceOpenRequest, SurfaceTabRow, SurfaceTarget,
};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{SurfaceBacking, TabBacking};
use crate::service::team::{
    apply_team_attachment_under, authorize_team_attachment, AttachmentParent,
};
use crate::service::ServiceContext;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SurfaceList {
    pub declared: Vec<DeclaredSurfaceRow>,
    pub open: Vec<SurfaceTabRow>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SurfaceClosed {
    pub tab_id: String,
    pub warnings: Vec<String>,
}

impl ServiceContext {
    fn plugin_surface_facts(&self, plugin_id: &str) -> PluginSurfaceFacts {
        PluginSurfaceFacts {
            declared: self
                .surface_registry
                .get(plugin_id)
                .map(|declared| declared.to_vec()),
            view_registered: self.view_registry.get(plugin_id).is_some(),
            runtimes: self.surface_runtimes.runtime_names(),
        }
    }

    pub(crate) fn open_surface(
        &self,
        request: SurfaceOpenRequest,
    ) -> Result<SurfaceTabRow, SurfaceError> {
        let target = parse_surface_target(&request.target)?;
        let surface = resolve_surface_decl(&target, &self.plugin_surface_facts(&target.plugin_id))?;
        crate::service::tab::validate_tab_title(&surface.title).map_err(SurfaceError::Placement)?;
        let backing = SurfaceBacking {
            plugin_id: target.plugin_id.clone(),
            surface_id: target.surface_id.clone(),
            instance_id: uuid::Uuid::new_v4().simple().to_string(),
            owner_label: owner_label(&surface, request.title_from_owner),
            resume_url: None,
            resume_profile: None,
        };
        let row = {
            let mut mux = self.mux.write();
            place_surface_tab(&mut mux, &request, &target, &surface.title, backing)?
        };
        self.emit_state_changed();
        Ok(row)
    }

    pub(crate) fn list_surfaces(&self) -> SurfaceList {
        let mut declared = Vec::new();
        let runtimes = self.surface_runtimes.runtime_names();
        for plugin_id in self.surface_registry.ids() {
            let view_registered = self.view_registry.get(&plugin_id).is_some();
            let Some(surfaces) = self.surface_registry.get(&plugin_id) else {
                continue;
            };
            declared.extend(
                surfaces
                    .iter()
                    .filter(|surface| surface_renderable(surface, view_registered, &runtimes))
                    .map(|surface| DeclaredSurfaceRow::new(&plugin_id, surface)),
            );
        }
        declared.sort_by(|a, b| (&a.plugin_id, &a.surface_id).cmp(&(&b.plugin_id, &b.surface_id)));
        let mux = self.mux.read();
        let mut open: Vec<SurfaceTabRow> = mux
            .all_tabs()
            .filter_map(|tab| surface_tab_row(tab, mux.find_workspace_for_tab(&tab.id)))
            .collect();
        drop(mux);
        open.sort_by(|a, b| a.tab_id.cmp(&b.tab_id));
        SurfaceList { declared, open }
    }

    pub(crate) fn close_surface(
        &self,
        caller_tab_id: Option<&str>,
        tab_id: &str,
    ) -> Result<SurfaceClosed, SurfaceError> {
        let resolved = {
            let mux = self.mux.read();
            let resolved = mux
                .resolve_tab_id(tab_id)
                .ok_or_else(|| SurfaceError::TabNotFound(tab_id.to_string()))?;
            let tab = mux
                .get_tab(&resolved)
                .ok_or_else(|| SurfaceError::TabNotFound(tab_id.to_string()))?;
            if tab.surface().is_none() {
                return Err(SurfaceError::NotASurface(resolved));
            }
            resolved
        };
        let warnings = self
            .close_tab_guarded(caller_tab_id, &resolved, false)
            .map_err(SurfaceError::Placement)?;
        Ok(SurfaceClosed {
            tab_id: resolved,
            warnings,
        })
    }
}

fn place_surface_tab(
    mux: &mut MultiplexerState,
    request: &SurfaceOpenRequest,
    target: &SurfaceTarget,
    title: &str,
    backing: SurfaceBacking,
) -> Result<SurfaceTabRow, SurfaceError> {
    let explicit_owner = match request.owner_tab_id.as_deref() {
        Some(raw) => Some(
            mux.resolve_tab_id(raw)
                .ok_or_else(|| SurfaceError::OwnerNotFound(raw.to_string()))?,
        ),
        None => None,
    };
    let caller = request
        .caller_tab_id
        .as_deref()
        .and_then(|raw| mux.resolve_tab_id(raw))
        .and_then(|id| mux.get_tab(&id));
    let owner_id = choose_owner(explicit_owner.as_deref(), caller).map(str::to_string);
    let owner = match owner_id.as_deref() {
        Some(id) => {
            let owner = mux
                .get_tab(id)
                .ok_or_else(|| SurfaceError::OwnerNotFound(id.to_string()))?;
            if owner.surface().is_some() {
                return Err(SurfaceError::OwnerIsSurface(id.to_string()));
            }
            Some(owner)
        }
        None => None,
    };
    let placement = surface_placement(owner);
    let owner_workspace = owner_id
        .as_deref()
        .and_then(|id| mux.find_workspace_for_tab(id));
    let workspace_id = match (request.workspace_id.clone(), owner_workspace) {
        (Some(requested), Some(owner_workspace)) if requested != owner_workspace => {
            return Err(SurfaceError::WorkspaceMismatch {
                requested,
                owner_workspace,
            });
        }
        (requested, owner_workspace) => crate::service::spawn::resolve_spawn_workspace_id(
            requested.or(owner_workspace),
            None,
            request.caller_tab_id.as_deref(),
            |tab| mux.find_workspace_for_tab(tab),
            mux.get_active_workspace()
                .map(|workspace| workspace.id.clone()),
            mux.list_workspaces()
                .first()
                .map(|workspace| workspace.id.clone()),
        )
        .map_err(SurfaceError::Placement)?,
    };
    let team_attachment = match (placement.team_id.as_deref(), owner_id.as_deref()) {
        (Some(team_id), Some(owner)) => Some(
            authorize_team_attachment(mux, team_id, owner, &workspace_id)
                .map_err(SurfaceError::Placement)?,
        ),
        _ => None,
    };
    let tab_id = mux
        .create_tab(&workspace_id)
        .map_err(SurfaceError::Placement)?;
    if let Err(error) = mux.rename_tab(&tab_id, title) {
        mux.rollback_spawn_tab(&workspace_id, &tab_id);
        return Err(SurfaceError::Placement(error));
    }
    let tab = mux
        .get_tab_mut(&tab_id)
        .ok_or_else(|| SurfaceError::Placement(format!("Tab {tab_id} vanished while opening")))?;
    tab.backing = TabBacking::Surface(backing);
    tab.parent_tab_id = placement.parent_tab_id;
    tab.spawned_by_tab_id = owner_id.clone();
    tab.depth = placement.depth;
    if let (Some(attachment), Some(owner)) = (team_attachment.as_ref(), owner_id.as_deref()) {
        let parent = AttachmentParent::Owner(owner);
        if let Err(error) = apply_team_attachment_under(mux, attachment, &tab_id, parent) {
            mux.rollback_spawn_tab(&workspace_id, &tab_id);
            return Err(SurfaceError::Placement(error));
        }
    }
    let tab = mux
        .get_tab(&tab_id)
        .ok_or_else(|| SurfaceError::Placement(format!("Tab {tab_id} vanished while opening")))?;
    let row = surface_tab_row(tab, Some(workspace_id)).ok_or_else(|| {
        SurfaceError::Placement(format!(
            "Tab {tab_id} lost its {}:{} backing",
            target.plugin_id, target.surface_id
        ))
    })?;
    Ok(row)
}
