use crate::service_plan::PlanError;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

pub(in crate::service_plan) struct NewBodyFile {
    path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl NewBodyFile {
    pub(super) fn write(path: PathBuf, body: &str) -> Result<Self, PlanError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| PlanError::Io(error.to_string()))?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| PlanError::Io(error.to_string()))?;
        let mut pending = Self {
            path,
            file: Some(file),
            committed: false,
        };
        if let Some(file) = pending.file.as_mut() {
            file.write_all(body.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|error| PlanError::Io(error.to_string()))?;
        }
        Ok(pending)
    }

    pub(super) fn keep(&mut self) {
        self.file.take();
        self.committed = true;
    }
}

impl Drop for NewBodyFile {
    fn drop(&mut self) {
        self.file.take();
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
