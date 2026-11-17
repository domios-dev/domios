use codeterm_util::{
    fs_links::{file_identity, identity, FileIdentity},
    project_roots::canonical_target,
};
use std::{
    collections::HashSet,
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Default)]
pub(super) struct Isolation {
    roots: Vec<PathBuf>,
    retained: Vec<(FileIdentity, Arc<File>)>,
}

impl Isolation {
    pub fn identities(&self) -> Vec<FileIdentity> {
        self.retained.iter().map(|(id, _)| *id).collect()
    }
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub fn extend(&mut self, roots: &[PathBuf], own: &[PathBuf]) -> Result<(), String> {
        for root in roots {
            let target =
                canonical_target(root).ok_or("Isolated account path cannot be resolved")?;
            if own.iter().any(|own| target.starts_with(own)) || self.roots.contains(root) {
                continue;
            }
            if self.roots.len() >= 1024 {
                return Err("Isolated account file bound exceeded".into());
            }
            self.roots.push(root.clone());
        }
        let files = self.files()?;
        for path in files {
            let file = match File::open(path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.to_string()),
            };
            let id = identity(&file).map_err(|error| error.to_string())?;
            if !self.retained.iter().any(|(held, _)| *held == id) {
                if self.retained.len() >= 1024 {
                    return Err("Isolated account file bound exceeded".into());
                }
                self.retained.push((id, Arc::new(file)));
            }
        }
        Ok(())
    }

    pub fn denies(&self, path: &Path) -> Result<bool, String> {
        let Some(id) = file_identity(path).map_err(|error| error.to_string())? else {
            return Ok(false);
        };
        if self.retained.iter().any(|(held, _)| *held == id) {
            return Ok(true);
        }
        for path in self.files()? {
            if file_identity(&path).map_err(|error| error.to_string())? == Some(id) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn files(&self) -> Result<Vec<PathBuf>, String> {
        let mut pending = self.roots.clone();
        let mut files = Vec::new();
        let mut visited = HashSet::new();
        let mut count = 0;
        while let Some(path) = pending.pop() {
            count += 1;
            if count > 1024 {
                return Err("Isolated account file bound exceeded".into());
            }
            let metadata = match std::fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.to_string()),
            };
            if metadata.is_file() {
                files.push(path);
            } else if metadata.is_dir()
                && visited
                    .insert(canonical_target(&path).ok_or("Isolated directory cannot be resolved")?)
            {
                for entry in std::fs::read_dir(path).map_err(|error| error.to_string())? {
                    pending.push(entry.map_err(|error| error.to_string())?.path());
                }
            }
        }
        Ok(files)
    }
}
