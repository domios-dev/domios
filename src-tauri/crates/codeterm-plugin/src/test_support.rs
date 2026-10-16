use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[cfg(test)]
#[path = "test_support/tests.rs"]
mod tests;

pub(crate) fn fixture_repo_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        find_repo_root(fixture_manifest_dir())
            .expect("repository containing plugins/channel.json is required for fixtures")
    })
}

fn find_repo_root(manifest_dir: &Path) -> Option<PathBuf> {
    manifest_dir
        .ancestors()
        .find(|root| root.join("plugins/channel.json").is_file())
        .map(Path::to_path_buf)
}

pub(crate) fn fixture_manifest_dir() -> &'static Path {
    static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
    DIRECTORY.get_or_init(|| {
        std::env::var_os("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
    })
}
