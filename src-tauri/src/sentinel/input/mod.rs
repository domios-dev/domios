mod convert;
mod materialize;

use std::path::PathBuf;
use std::time::Instant;

use codeterm_types::sentinel_input::{DataRef, InputSpec};

pub(super) use convert::convert;
pub(super) use materialize::{materialize, sweep_orphans};

#[derive(Debug)]
pub(super) struct InputFiles {
    pub source: PathBuf,
}

pub(super) struct Cleanup(std::sync::Mutex<Option<PathBuf>>);

impl Cleanup {
    pub(super) fn new(path: PathBuf) -> Self {
        Self(std::sync::Mutex::new(Some(path)))
    }

    pub(super) fn settle(&self) {
        let Some(path) = self.0.lock().expect("cleanup mutex").take() else {
            return;
        };
        if let Err(error) = std::fs::remove_dir_all(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::error!(
                    "[blueprint-input] cleanup failed for {}: {error}",
                    path.display()
                );
            }
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        self.settle();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blueprint_input_cleanup_removes_run_directory_on_drop() {
        let temp = tempfile::tempdir().unwrap();
        let run = temp.path().join("run");
        std::fs::create_dir_all(&run).unwrap();
        drop(Cleanup::new(run.clone()));
        assert!(!run.exists());
    }
}

pub(super) fn prepare(
    data_dir: &std::path::Path,
    blueprint_id: &str,
    run_id: &str,
    spec: &InputSpec,
    deadline: Instant,
) -> Result<InputFiles, String> {
    materialize(data_dir, blueprint_id, run_id, &spec.data, deadline)
}
