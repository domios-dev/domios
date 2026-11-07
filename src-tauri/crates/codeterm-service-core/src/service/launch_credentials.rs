use std::fs;
use std::path::{Path, PathBuf};

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
        match fs::remove_dir_all(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("remove stale launch credentials: {error}")),
        }
        codeterm_util::private_file::create_dir_owner_only(&self.path)
            .map_err(|error| format!("create launch credential directory: {error}"))
    }

    pub(crate) fn file_path(&self, prefix: &str, suffix: &str) -> PathBuf {
        self.path
            .join(format!("{prefix}{}{suffix}", uuid::Uuid::new_v4()))
    }
}

#[cfg(test)]
mod tests;
