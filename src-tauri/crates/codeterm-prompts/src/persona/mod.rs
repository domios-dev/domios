//! Delivery-time composition of role presets: which occasion carries which
//! fragment, and how both are inspected.

pub mod inspect;
pub mod occasion;

#[cfg(test)]
mod tests;

use crate::role_preset::{self, RolePreset};
use codeterm_types::agent::AgentRole;
use codeterm_types::team::role_registration::RoleRegistration;

pub use occasion::Occasion;

/// Resolve a preset against the Team's registrations, falling back to the default package.
pub fn resolve(
    team: &[RoleRegistration],
    role: Option<AgentRole>,
    role_profile: Option<&str>,
    task_guidance: Option<&str>,
) -> Option<RolePreset> {
    role_preset::resolve_in(team, role, role_profile, task_guidance)
}

/// The text one occasion contributes, with no payload attached. Unmatched
/// pairs land on the occasion's declared fallback.
pub fn guidance_for(
    occasion: Occasion,
    team: &[RoleRegistration],
    role: Option<AgentRole>,
    role_profile: Option<&str>,
    task_guidance: Option<&str>,
) -> String {
    let preset = resolve(team, role, role_profile, task_guidance);
    occasion::guidance(occasion, preset.as_ref()).0
}

/// One delivery through the shared seam: resolve, then let the occasion table
/// place the guidance next to the occasion's payload.
pub fn deliver(
    occasion: Occasion,
    team: &[RoleRegistration],
    role: Option<AgentRole>,
    role_profile: Option<&str>,
    task_guidance: Option<&str>,
    payload: Option<&str>,
) -> Option<String> {
    let preset = resolve(team, role, role_profile, task_guidance);
    occasion::compose(occasion, preset.as_ref(), payload)
}
