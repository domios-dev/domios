//! Re-export — implementation lives in `crates/codeterm-paths`.
pub use codeterm_paths::*;

use std::sync::OnceLock;

static INSTANCE_ID: OnceLock<String> = OnceLock::new();

pub fn codeterm_instance_id() -> &'static str {
    INSTANCE_ID.get_or_init(|| crate::mesh::peers::load_or_create_identity().instance_id)
}
