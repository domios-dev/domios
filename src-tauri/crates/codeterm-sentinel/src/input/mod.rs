mod convert;
mod materialize;

use std::path::PathBuf;
use std::time::Instant;

use codeterm_blueprint_types::sentinel_input::InputSpec;

pub use convert::convert;
pub use materialize::{materialize, sweep_orphans};

#[derive(Debug)]
pub struct InputFiles {
    pub source: PathBuf,
}

pub struct Cleanup(std::sync::Mutex<Option<PathBuf>>);

impl Cleanup {
    pub fn new(path: PathBuf) -> Self {
        Self(std::sync::Mutex::new(Some(path)))
    }

    pub fn settle(&self) {
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

pub fn prepare(
    data_dir: &std::path::Path,
    blueprint_id: &str,
    run_id: &str,
    spec: &InputSpec,
    deadline: Instant,
) -> Result<InputFiles, String> {
    materialize(data_dir, blueprint_id, run_id, &spec.data, deadline)
}

pub fn text_output(
    job: &super::store::Sentinel,
    deadline: Instant,
) -> Result<super::probe::Output, String> {
    use std::io::Read;
    let spec = job.input().ok_or("input source missing")?;
    let run_id = uuid::Uuid::new_v4().simple().to_string();
    let data_dir = codeterm_paths::data_dir();
    let run_dir = codeterm_blueprint_types::sentinel_input::run_dir(&data_dir, &job.id, &run_id)?;
    let _cleanup = Cleanup::new(run_dir);
    let files = prepare(&data_dir, &job.id, &run_id, spec, deadline)?;
    let mut bytes = Vec::new();
    std::fs::File::open(files.source)
        .map_err(|error| error.to_string())?
        .take((codeterm_blueprint_types::blueprint_terminal::MAX_PAYLOAD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.is_empty()
        || bytes.len() > codeterm_blueprint_types::blueprint_terminal::MAX_PAYLOAD_BYTES
    {
        return Err("input text must contain 1..32768 UTF-8 bytes".into());
    }
    let text = String::from_utf8(bytes.clone()).map_err(|_| "input text is not UTF-8")?;
    Ok(super::probe::Output {
        text,
        bytes,
        success: true,
        exit_code: Some(0),
        ..Default::default()
    })
}
