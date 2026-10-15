use crate::multiplexer::types::Tab;
pub(crate) use codeterm_surface::surface::model::{
    owner_label, parse_surface_target, resolve_surface_decl, surface_renderable,
    PluginSurfaceFacts, SurfaceTarget,
};
pub use codeterm_surface::surface::model::{
    DeclaredSurfaceRow, SurfaceError, SurfaceOpenRequest, SurfaceTabRow,
};

/// A tab that may own surfaces by default: one that carries an agent identity.
pub(crate) fn tab_has_owner_identity(tab: &Tab) -> bool {
    tab.agent_type.is_some() || tab.is_chat_backend()
}

/// Explicit owner wins; otherwise the caller owns the surface only when it is an agent.
pub(crate) fn choose_owner<'a>(
    explicit: Option<&'a str>,
    caller: Option<&'a Tab>,
) -> Option<&'a str> {
    explicit.or_else(|| {
        caller
            .filter(|tab| tab_has_owner_identity(tab))
            .map(|tab| tab.id.as_str())
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SurfacePlacement {
    pub parent_tab_id: Option<String>,
    pub team_id: Option<String>,
    pub depth: u8,
}

/// Nest under the owner and join its Team, so Team close tears the surface down.
pub(crate) fn surface_placement(owner: Option<&Tab>) -> SurfacePlacement {
    owner
        .map(|owner| SurfacePlacement {
            parent_tab_id: Some(owner.id.clone()),
            team_id: owner.team_id.clone(),
            depth: owner.depth.saturating_add(1),
        })
        .unwrap_or_default()
}

pub(crate) fn surface_tab_row(tab: &Tab, workspace_id: Option<String>) -> Option<SurfaceTabRow> {
    let surface = tab.surface()?;
    Some(SurfaceTabRow {
        tab_id: tab.id.clone(),
        workspace_id,
        plugin_id: surface.plugin_id.clone(),
        surface_id: surface.surface_id.clone(),
        instance_id: surface.instance_id.clone(),
        title: tab.title.clone(),
        owner_tab_id: tab.parent_tab_id.clone(),
    })
}
