use std::path::{Path, PathBuf};

pub fn existing_input(path: &Path) -> PathBuf {
    // Cargo treats a missing rerun-if-changed input as dirty on every invocation.
    path.ancestors()
        .find(|candidate| candidate.exists())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}
