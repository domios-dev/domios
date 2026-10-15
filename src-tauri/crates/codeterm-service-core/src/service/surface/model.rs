use codeterm_plugin::surfaces::{SurfaceDecl, SurfaceRenderer, SurfaceRestore};
use serde::{Deserialize, Serialize};

use crate::multiplexer::types::Tab;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceError {
    InvalidTarget(String),
    PluginUnavailable(String),
    SurfaceNotDeclared {
        plugin_id: String,
        surface_id: String,
    },
    ViewUnavailable(String),
    OwnerNotFound(String),
    OwnerIsSurface(String),
    WorkspaceMismatch {
        requested: String,
        owner_workspace: String,
    },
    NotASurface(String),
    NotAStreamSurface(String),
    RuntimeUnavailable(String),
    TabNotFound(String),
    Placement(String),
}

impl SurfaceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidTarget(_) => "invalid_surface_target",
            Self::PluginUnavailable(_) => "surface_plugin_unavailable",
            Self::SurfaceNotDeclared { .. } => "surface_not_declared",
            Self::ViewUnavailable(_) => "surface_view_unavailable",
            Self::OwnerNotFound(_) => "surface_owner_not_found",
            Self::OwnerIsSurface(_) => "surface_owner_is_surface",
            Self::WorkspaceMismatch { .. } => "surface_workspace_mismatch",
            Self::NotASurface(_) => "not_a_surface",
            Self::NotAStreamSurface(_) => "not_a_stream_surface",
            Self::RuntimeUnavailable(_) => "surface_runtime_unavailable",
            Self::TabNotFound(_) => "tab_not_found",
            Self::Placement(_) => "surface_placement_failed",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidTarget(raw) => {
                format!("'{raw}' is not <plugin_id>:<surface_id>")
            }
            Self::PluginUnavailable(plugin) => format!(
                "plugin {plugin} is not installed and enabled, or declares no surfaces it can render"
            ),
            Self::SurfaceNotDeclared {
                plugin_id,
                surface_id,
            } => format!("plugin {plugin_id} declares no surface {surface_id}"),
            Self::ViewUnavailable(plugin) => {
                format!("plugin {plugin} has no registered view to render the surface")
            }
            Self::OwnerNotFound(owner) => format!("owner Tab {owner} not found"),
            Self::OwnerIsSurface(owner) => {
                format!("owner Tab {owner} is itself a surface; pick its owner instead")
            }
            Self::WorkspaceMismatch {
                requested,
                owner_workspace,
            } => format!(
                "workspace {requested} differs from the owner's workspace {owner_workspace}"
            ),
            Self::NotASurface(tab) => {
                format!("Tab {tab} is not a surface tab; close it with `codeterm tab close`")
            }
            Self::NotAStreamSurface(tab) => {
                format!("Tab {tab} is not a stream surface; it has no frames or runtime calls")
            }
            Self::RuntimeUnavailable(runtime) => {
                format!("no core surface runtime named {runtime} is registered")
            }
            Self::TabNotFound(tab) => format!("Tab {tab} not found"),
            Self::Placement(detail) => detail.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SurfaceOpenRequest {
    pub target: String,
    #[serde(default)]
    pub owner_tab_id: Option<String>,
    #[serde(default)]
    pub caller_tab_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub title_from_owner: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SurfaceTabRow {
    pub tab_id: String,
    pub workspace_id: Option<String>,
    pub plugin_id: String,
    pub surface_id: String,
    pub instance_id: String,
    pub title: String,
    pub owner_tab_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeclaredSurfaceRow {
    pub plugin_id: String,
    pub surface_id: String,
    pub title: String,
    pub renderer: SurfaceRenderer,
    pub agent_bound: bool,
    pub restore: SurfaceRestore,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chrome: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl DeclaredSurfaceRow {
    pub(crate) fn new(plugin_id: &str, surface: &SurfaceDecl) -> Self {
        Self {
            plugin_id: plugin_id.to_string(),
            surface_id: surface.id.clone(),
            title: surface.title.clone(),
            renderer: surface.renderer,
            agent_bound: surface.agent_bound,
            restore: surface.restore,
            runtime: surface.stream_runtime().map(str::to_string),
            chrome: surface.chrome.clone(),
            icon: surface.icon.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceTarget {
    pub plugin_id: String,
    pub surface_id: String,
}

pub(crate) fn parse_surface_target(raw: &str) -> Result<SurfaceTarget, SurfaceError> {
    let invalid = || SurfaceError::InvalidTarget(raw.to_string());
    let (plugin_id, surface_id) = raw.trim().split_once(':').ok_or_else(invalid)?;
    if plugin_id.is_empty() || surface_id.is_empty() || surface_id.contains(':') {
        return Err(invalid());
    }
    Ok(SurfaceTarget {
        plugin_id: plugin_id.to_string(),
        surface_id: surface_id.to_string(),
    })
}

/// What the live registries know about the plugin a surface open names.
#[derive(Debug, Clone, Default)]
pub(crate) struct PluginSurfaceFacts {
    pub declared: Option<Vec<SurfaceDecl>>,
    pub view_registered: bool,
    pub runtimes: Vec<String>,
}

/// Whether core can render `surface` now: its view is registered, or its stream runtime exists.
pub(crate) fn surface_renderable(
    surface: &SurfaceDecl,
    view_registered: bool,
    runtimes: &[String],
) -> bool {
    match surface.stream_runtime() {
        Some(runtime) => runtimes.iter().any(|known| known == runtime),
        None => view_registered,
    }
}

pub(crate) fn resolve_surface_decl(
    target: &SurfaceTarget,
    facts: &PluginSurfaceFacts,
) -> Result<SurfaceDecl, SurfaceError> {
    let not_declared = || SurfaceError::SurfaceNotDeclared {
        plugin_id: target.plugin_id.clone(),
        surface_id: target.surface_id.clone(),
    };
    let Some(declared) = facts.declared.as_ref() else {
        return Err(if facts.view_registered {
            not_declared()
        } else {
            SurfaceError::PluginUnavailable(target.plugin_id.clone())
        });
    };
    let surface = declared
        .iter()
        .find(|surface| surface.id == target.surface_id)
        .ok_or_else(not_declared)?;
    if surface_renderable(surface, facts.view_registered, &facts.runtimes) {
        return Ok(surface.clone());
    }
    Err(match (surface.renderer, surface.stream_runtime()) {
        (SurfaceRenderer::Stream, Some(runtime)) => {
            SurfaceError::RuntimeUnavailable(runtime.to_string())
        }
        _ => SurfaceError::ViewUnavailable(target.plugin_id.clone()),
    })
}

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

pub(crate) fn owner_label(surface: &SurfaceDecl, title_from_owner: bool) -> bool {
    surface.agent_bound || title_from_owner
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
