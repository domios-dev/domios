use std::sync::OnceLock;

use codeterm_team_package::{builtin, CheckOptions, Package};
use codeterm_types::agent::role_kind::builtins;
use codeterm_types::team::role_registration::PinnedPackage;

pub const DEFAULT_PACKAGE: &str = "default";

/// The builtin default package; `builtin_packages_are_valid_folder_packages` keeps it valid.
pub fn default_package() -> &'static Package {
    static PACKAGE: OnceLock<Package> = OnceLock::new();
    PACKAGE.get_or_init(|| {
        builtin::check(DEFAULT_PACKAGE, &CheckOptions::domios())
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

pub fn builtin_prose(kind_id: &str) -> Option<&'static str> {
    default_package().role_instructions(prose_role(kind_id))
}

/// Builtin worker names and descriptions, in package order.
pub fn builtin_workers() -> Vec<(String, Option<String>)> {
    default_package()
        .document
        .workers
        .iter()
        .map(|(name, role)| (name.to_string(), role.description.clone()))
        .collect()
}
