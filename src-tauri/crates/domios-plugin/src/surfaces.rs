//! Plugin-declared app tabs; the registry holds only enabled plugins' declarations.

use std::sync::Arc;

use crate::capability::CapabilityRegistry;

pub use crate::js_runtime::manifest::{SurfaceDecl, SurfaceRenderer, SurfaceRestore};

pub type SurfaceRegistry = CapabilityRegistry<Vec<SurfaceDecl>>;
pub type SurfaceRegistryHandle = Arc<SurfaceRegistry>;
