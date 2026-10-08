use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::Capabilities;

const MAX_SURFACES: usize = 8;
const MAX_SURFACE_ID_BYTES: usize = 32;
const MAX_SURFACE_TITLE_CHARS: usize = 32;

/// How core renders a surface tab. Only the sandboxed plugin view exists today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SurfaceRenderer {
    View,
}

impl SurfaceRenderer {
    fn required_capability(self, caps: &Capabilities) -> Option<&'static str> {
        match self {
            Self::View => (!caps.view).then_some("view"),
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
