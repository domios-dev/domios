//! Re-export — implementation lives in `crates/codeterm-paths`.
pub use codeterm_paths::*;

use std::path::Path;
use std::sync::OnceLock;

static INSTANCE_ID: OnceLock<String> = OnceLock::new();

pub fn codeterm_instance_id() -> &'static str {
    INSTANCE_ID
        .get_or_init(|| stable_instance_id_for_path(&codeterm_paths::data_dir()))
}

fn stable_instance_id_for_path(path: &Path) -> String {
    let normalized = std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    let normalized = if cfg!(windows) {
        normalized.to_lowercase()
    } else {
        normalized
    };
    let hash = normalized.as_bytes().iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("instance-{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_identity_is_stable_for_a_data_directory_and_differs_between_directories() {
        assert_eq!(
            stable_instance_id_for_path(Path::new("/data/codeterm-dev")),
            stable_instance_id_for_path(Path::new("/data/codeterm-dev"))
        );
        assert_ne!(
            stable_instance_id_for_path(Path::new("/data/codeterm-dev")),
            stable_instance_id_for_path(Path::new("/data/codeterm"))
        );
    }
}
