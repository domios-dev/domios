use std::sync::OnceLock;

use codeterm_team_package::{builtin, CheckOptions, Package};
use codeterm_types::agent::role_kind::builtins;
use codeterm_types::team::role_registration::{PinnedPackage, RoleRegistration};

pub const DEFAULT_PACKAGE: &str = "default";

/// The builtin default package; `builtin_packages_are_valid_folder_packages` keeps it valid.
pub fn default_package() -> &'static Package {
    static PACKAGE: OnceLock<Package> = OnceLock::new();
    PACKAGE.get_or_init(|| {
        builtin::check(DEFAULT_PACKAGE, &CheckOptions::default())
            .and_then(|checked| checked.package)
            .expect("the builtin default package passes team check")
    })
}

pub fn default_identity() -> PinnedPackage {
    let document = &default_package().document;
    PinnedPackage {
        name: document.name.clone(),
        version: document.version.clone(),
    }
}

/// The package role whose prose a kind reads; the host-only Watcher reads the Planner's.
pub fn prose_role(kind_id: &str) -> &str {
    if kind_id == builtins::WATCHER {
        builtins::PLANNER
    } else {
        kind_id
    }
}

/// A role's prose in the builtin default package.
pub fn builtin_prose(kind_id: &str) -> Option<&'static str> {
    default_package().role_instructions(prose_role(kind_id))
}

/// The default package as a whole, as the registrations of `team_id`. A Team with no
/// registrations is exactly this Team: host policy, never a per-role fallback.
pub fn default_registrations(team_id: &str) -> Vec<RoleRegistration> {
    let package = default_package();
    let identity = default_identity();
    package
        .document
        .roles()
        .filter_map(|(name, role)| {
            Some(RoleRegistration {
                team_id: team_id.to_string(),
                kind: role.kind,
                name: name.to_string(),
                label: role.name.clone().unwrap_or_else(|| name.to_string()),
                description: role.description.clone(),
                start: role.effective_start(name),
                package: identity.clone(),
                instructions: package.role_instructions(name)?.to_string(),
                model: role.model.clone(),
                reasoning: role.reasoning.clone(),
                consent_decision_id: None,
                files: Vec::new(),
                skills: role.skills.clone(),
                registered_at: 0,
            })
        })
        .collect()
}
