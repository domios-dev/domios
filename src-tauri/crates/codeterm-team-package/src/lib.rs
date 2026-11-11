mod archive;
pub mod builtin;
pub mod contracts;
mod diagnostics;
mod document;
mod image;
mod ordered_map;
mod package;
mod parse;
mod paths;
mod payload;
mod scalar;
mod serialize;
mod validate;
mod yaml;

pub use diagnostics::{DiagnosticCode, DiagnosticSeverity, PackageCheckReport, PackageDiagnostic};
pub use document::*;
pub use image::{check_icon, IconKind};
pub use ordered_map::OrderedMap;
pub use package::{
    check_archive, check_entries, check_folder, check_path, check_yaml, format_of,
    frontmatter_name, Checked, Package, PackageFormat,
};
pub use payload::{is_executable, payload_items, PayloadKind, TemplatePayloadItem};
pub use validate::{
    is_harness, is_model_id, is_package_name, is_role_name, is_semver, schedule_window,
};

/// Explicit size and count bounds; every form is read within them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    pub max_yaml_bytes: usize,
    pub max_string_bytes: usize,
    pub max_depth: usize,
    pub max_items: usize,
    pub max_nodes: usize,
    pub max_entries: usize,
    pub max_entry_bytes: usize,
    pub max_total_bytes: usize,
    pub max_path_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_yaml_bytes: 8 * 1024 * 1024,
            max_string_bytes: 2 * 1024 * 1024,
            max_depth: 32,
            max_items: 1024,
            max_nodes: 200_000,
            max_entries: 512,
            max_entry_bytes: 16 * 1024 * 1024,
            max_total_bytes: 64 * 1024 * 1024,
            max_path_bytes: 255,
        }
    }
}

/// What the checking host provides beyond the format: the builtin worker names roles may inherit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostProfile {
    pub builtin_workers: Vec<String>,
}

impl HostProfile {
    pub fn domios() -> Self {
        Self {
            builtin_workers: ["worker", "qa", "reviewer", "release_manager"]
                .map(str::to_string)
                .to_vec(),
        }
    }
}

/// Validator inputs: bounds, and the host profile when a host runs the check.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckOptions {
    pub limits: Limits,
    pub host: Option<HostProfile>,
}

impl CheckOptions {
    /// The reference validator as Domios runs it.
    pub fn domios() -> Self {
        Self {
            limits: Limits::default(),
            host: Some(HostProfile::domios()),
        }
    }
}

/// The JSON Schema of `team.yaml`, generated from [`TeamDocument`].
pub fn schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(TeamDocument)).unwrap_or_default()
}

#[cfg(test)]
mod tests;
