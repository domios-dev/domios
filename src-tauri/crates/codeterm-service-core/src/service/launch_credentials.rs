use std::fs;
use std::path::{Path, PathBuf};

/// Protects against other users and harness environment scrubbing.
/// Writers of the daemon data directory, including same-user/root rename or mount operations, are owner-equivalent and out of scope.
#[derive(Clone)]
pub struct Directory {
    path: PathBuf,
}

impl Directory {
    pub fn reset(data_dir: &Path) -> Result<Self, String> {
        let directory = Self::resolve(data_dir)?;
        directory.clear_and_create()?;
        Ok(directory)
    }

    fn resolve(data_dir: &Path) -> Result<Self, String> {
        let runtime = data_dir.join("runtime");
        fs::create_dir_all(&runtime)
            .map_err(|error| format!("create runtime directory: {error}"))?;
        let parent = fs::canonicalize(runtime)
            .map_err(|error| format!("resolve runtime directory: {error}"))?;
        Ok(Self {
            path: parent.join("launch-credentials"),
        })
    }

    fn clear_and_create(&self) -> Result<(), String> {
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) => {
                if !metadata.is_dir() || codeterm_util::safe_tree::is_link(&metadata) {
                    return Err("launch credential root must be an ordinary directory, not a link or junction".into());
                }
                codeterm_util::private_file::open_directory_owner_only(&self.path)
                    .map_err(|error| format!("validate launch credential directory: {error}"))?;
                for entry in fs::read_dir(&self.path)
                    .map_err(|error| format!("read launch credential directory: {error}"))?
                {
                    let entry =
                        entry.map_err(|error| format!("read launch credential entry: {error}"))?;
                    let path = entry.path();
                    let metadata = fs::symlink_metadata(&path)
                        .map_err(|error| format!("inspect launch credential entry: {error}"))?;
                    if !metadata.is_file() || codeterm_util::safe_tree::is_link(&metadata) {
                        return Err(format!("unexpected non-regular launch credential entry {}; directories, links, junctions and mounts are refused", path.display()));
                    }
                    fs::remove_file(&path).map_err(|error| {
                        format!("remove stale launch credential {}: {error}", path.display())
                    })?;
                }
                fs::remove_dir(&self.path).map_err(|error| {
                    format!("remove empty launch credential directory: {error}")
                })?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("inspect launch credential directory: {error}")),
        }
        codeterm_util::private_file::create_dir_owner_only(&self.path)
            .map_err(|error| format!("create launch credential directory: {error}"))
    }

    pub(crate) fn file_path(&self, prefix: &str, suffix: &str) -> PathBuf {
        self.path()
            .join(format!("{prefix}{}{suffix}", uuid::Uuid::new_v4()))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests;
