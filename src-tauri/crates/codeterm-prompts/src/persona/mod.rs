//! Delivery-time composition of role presets: which occasion carries which
//! fragment, which leaf kinds disk overlays may add, and how both are inspected.

pub mod inspect;
pub mod occasion;
pub mod overlay;

#[cfg(test)]
mod tests;

use crate::role_preset::{self, RolePreset};
use codeterm_types::agent::AgentRole;

pub use occasion::Occasion;

/// Resolve a preset against the installed overlay catalog.
pub fn resolve_ambient(
    role: Option<AgentRole>,
    role_profile: Option<&str>,
    task_guidance: Option<&str>,
) -> Option<RolePreset> {
    role_preset::resolve_in(
        overlay::ambient().as_ref(),
        role,
        role_profile,
        task_guidance,
    )
}

/// The text one occasion contributes, with no payload attached. Unmatched
/// pairs land on the occasion's declared fallback.
pub fn guidance_for(
    occasion: Occasion,
    role: Option<AgentRole>,
    role_profile: Option<&str>,
    task_guidance: Option<&str>,
) -> String {
    let preset = resolve_ambient(role, role_profile, task_guidance);
    occasion::guidance(occasion, preset.as_ref()).0
}

/// One delivery through the shared seam: resolve, then let the occasion table
/// place the guidance next to the occasion's payload.
pub fn deliver(
    occasion: Occasion,
    role: Option<AgentRole>,
    role_profile: Option<&str>,
    task_guidance: Option<&str>,
    payload: Option<&str>,
) -> Option<String> {
    let preset = resolve_ambient(role, role_profile, task_guidance);
    occasion::compose(occasion, preset.as_ref(), payload)
}
