use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::Capabilities;

const MAX_SURFACES: usize = 8;
const MAX_SURFACE_ID_BYTES: usize = 32;
const MAX_SURFACE_TITLE_CHARS: usize = 32;
const MAX_SURFACE_ICON_BYTES: usize = 40;

/// How core renders a surface tab: the sandboxed plugin view, or frames from a core runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SurfaceRenderer {
    View,
    Stream,
}

impl SurfaceRenderer {
    fn required_capability(self, caps: &Capabilities) -> Option<&'static str> {
        match self {
            Self::View => (!caps.view).then_some("view"),
            Self::Stream => None,
        }
    }
}

/// What a restored desk does with a surface tab.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SurfaceRestore {
    #[default]
    Relaunch,
    Placeholder,
    #[serde(rename = "none")]
    Discard,
}

/// A plugin-declared app tab (`capabilities.surfaces[]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SurfaceDecl {
    pub id: String,
    pub title: String,
    pub renderer: SurfaceRenderer,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub agent_bound: bool,
    #[serde(default)]
    pub restore: SurfaceRestore,
    /// The core surface runtime a `stream` surface runs on; core branches on it, never on the plugin id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    /// Toolbar spec core passes through to the frontend untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chrome: Option<serde_json::Value>,
    /// Lucide-style icon name for the surface's tab; an invalid name is dropped at load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl SurfaceDecl {
    /// The declared core runtime of a `stream` surface; `None` for a view surface.
    pub fn stream_runtime(&self) -> Option<&str> {
        match self.renderer {
            SurfaceRenderer::Stream => self.runtime.as_deref(),
            SurfaceRenderer::View => None,
        }
    }

    fn validate_renderer_fields(&self) -> Result<(), String> {
        match self.renderer {
            SurfaceRenderer::Stream => match self.runtime.as_deref() {
                Some(runtime) if valid_surface_id(runtime) => {}
                Some(runtime) => {
                    return Err(format!(
                        "surface '{}' runtime '{runtime}' must be 1-{MAX_SURFACE_ID_BYTES} lowercase letters, digits or '-'",
                        self.id
                    ))
                }
                None => {
                    return Err(format!(
                        "surface '{}' uses the stream renderer and must name a runtime",
                        self.id
                    ))
                }
            },
            SurfaceRenderer::View if self.runtime.is_some() || self.chrome.is_some() => {
                return Err(format!(
                    "surface '{}' is a view surface; runtime and chrome belong to stream surfaces",
                    self.id
                ));
            }
            SurfaceRenderer::View => {}
        }
        match &self.chrome {
            Some(chrome) if !chrome.is_object() => {
                Err(format!("surface '{}' chrome must be an object", self.id))
            }
            _ => Ok(()),
        }
    }
}

/// A lowercase kebab name such as `globe` or `layout-dashboard`.
fn valid_surface_icon(icon: &str) -> bool {
    !icon.is_empty()
        && icon.len() <= MAX_SURFACE_ICON_BYTES
        && !icon.starts_with('-')
        && !icon.ends_with('-')
        && !icon.contains("--")
        && icon
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_surface_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_SURFACE_ID_BYTES
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The tab-title invariant: one word, no whitespace or control characters.
fn valid_surface_title(title: &str) -> bool {
    !title.is_empty()
        && title.chars().count() <= MAX_SURFACE_TITLE_CHARS
        && !title
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
}

impl Capabilities {
    /// A bad icon only costs the tab its glyph, so it is dropped rather than failing the plugin.
    pub fn drop_invalid_surface_icons(&mut self, plugin_id: &str) {
        for surface in &mut self.surfaces {
            if let Some(icon) = surface.icon.take_if(|icon| !valid_surface_icon(icon)) {
                log::warn!(
                    "[plugin] {plugin_id}: surface '{}' icon '{icon}' must be a lowercase kebab name of at most {MAX_SURFACE_ICON_BYTES} characters; ignored",
                    surface.id
                );
            }
        }
    }

    pub fn surface(&self, surface_id: &str) -> Option<&SurfaceDecl> {
        self.surfaces
            .iter()
            .find(|surface| surface.id == surface_id)
    }

    /// Cross-field rules serde cannot express: bounded, unique surfaces whose renderer the plugin can back.
    pub fn validate_surfaces(&self) -> Result<(), String> {
        if self.surfaces.len() > MAX_SURFACES {
            return Err(format!(
                "surfaces declares {} entries; at most {MAX_SURFACES} are allowed",
                self.surfaces.len()
            ));
        }
        let mut seen = HashSet::new();
        for surface in &self.surfaces {
            if !valid_surface_id(&surface.id) {
                return Err(format!(
                    "surface id '{}' must be 1-{MAX_SURFACE_ID_BYTES} lowercase letters, digits or '-'",
                    surface.id
                ));
            }
            if !seen.insert(surface.id.as_str()) {
                return Err(format!("surface id '{}' is declared twice", surface.id));
            }
            if !valid_surface_title(&surface.title) {
                return Err(format!(
                    "surface '{}' title must be one word of 1-{MAX_SURFACE_TITLE_CHARS} characters without whitespace",
                    surface.id
                ));
            }
            surface.validate_renderer_fields()?;
            if let Some(capability) = surface.renderer.required_capability(self) {
                return Err(format!(
                    "surface '{}' uses a renderer that requires the {capability} capability",
                    surface.id
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
