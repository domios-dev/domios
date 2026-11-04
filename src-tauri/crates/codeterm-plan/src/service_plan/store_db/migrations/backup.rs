use super::PlanError;
use rusqlite::{Connection, DatabaseName};
use std::path::{Path, PathBuf};

pub(super) fn path(database: &Path) -> PathBuf {
    let mut name = database.as_os_str().to_owned();
    name.push(".pre-migration");
    PathBuf::from(name)
}

pub(super) fn create(connection: &Connection, database: &Path) -> Result<PathBuf, PlanError> {
    let backup = path(database);
    let staging = codeterm_util::atomic_write::unique_tmp_path(&backup);
    let copied = (|| -> Result<(), String> {
        connection
            .backup(DatabaseName::Main, &staging, None)
            .map_err(|error| error.to_string())?;
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&staging)
            .and_then(|file| file.sync_all())
            .map_err(|error| error.to_string())?;
        std::fs::rename(&staging, &backup).map_err(|error| error.to_string())
    })();
    if let Err(error) = copied {
        let _ = std::fs::remove_file(&staging);
        return Err(PlanError::Io(format!(
            "plan migration refused: backup to {} failed: {error}",
            backup.display()
        )));
    }
    Ok(backup)
}
