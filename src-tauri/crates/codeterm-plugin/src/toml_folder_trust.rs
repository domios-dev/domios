use std::path::Path;

use crate::provider::PreTrustOutcome;

fn record_plan(source: &str, cwd: &str, now: i64) -> Result<Option<String>, String> {
    let mut record: toml::Table =
        toml::from_str(source).map_err(|_| "invalid TOML trust record")?;
    let folders = record
        .entry("folders")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or("folders must be a table")?;
    let folder = folders
        .entry(cwd)
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or("folder must be a table")?;
    match folder.get("trusted") {
        Some(toml::Value::Boolean(true)) => return Ok(None),
        Some(toml::Value::Boolean(false)) | None => {}
        _ => return Err("trusted must be a boolean".into()),
    }
    folder.insert("trusted".into(), toml::Value::Boolean(true));
    folder.insert("decided_at".into(), toml::Value::Integer(now));
    toml::to_string(&record)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn read_record(path: &Path) -> Result<String, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= 1024 * 1024 => {
            std::fs::read_to_string(path).map_err(|error| error.to_string())
        }
        Ok(_) => Err("trust record must be a bounded regular file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn trusted(path: &Path, cwd: &str) -> Result<bool, String> {
    let source = read_record(path)?;
    record_plan(&source, cwd, 0).map(|edit| edit.is_none())
}

pub(crate) fn grant(path: &Path, cwd: &str) -> PreTrustOutcome {
    let result = (|| {
        let parent = path.parent().ok_or("trust record parent unavailable")?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let lock_path = path.with_extension("toml.lock");
        if lock_path.is_symlink() {
            return Err("trust lock cannot be a symlink".into());
        }
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|error| error.to_string())?;
        lock.try_lock().map_err(|error| error.to_string())?;
        let source = read_record(path)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs() as i64;
        let Some(output) = record_plan(&source, cwd, now)? else {
            return Ok(PreTrustOutcome::AlreadyTrusted);
        };
        if read_record(path)? != source {
            return Err("trust record changed during grant".into());
        }
        codeterm_util::atomic_write::write_atomic(path, output)
            .map_err(|error| error.to_string())?;
        Ok(PreTrustOutcome::Granted)
    })();
    result.unwrap_or_else(PreTrustOutcome::Failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_folder_trust_preserves_other_folders_and_fields() {
        let source = "extra = 'preserved'\n[folders.\"/other\"]\ntrusted = false\ndecided_at = 5\n";
        let cwd = "C:\\work\\quoted\"folder";
        let output = record_plan(source, cwd, 42).unwrap().unwrap();
        let parsed: toml::Value = toml::from_str(&output).unwrap();
        assert_eq!(parsed["folders"][cwd]["trusted"].as_bool(), Some(true));
        assert_eq!(parsed["folders"][cwd]["decided_at"].as_integer(), Some(42));
        assert_eq!(
            parsed["folders"]["/other"]["trusted"].as_bool(),
            Some(false)
        );
        assert_eq!(parsed["extra"].as_str(), Some("preserved"));
        assert_eq!(record_plan(&output, cwd, 43).unwrap(), None);
        assert!(record_plan("malformed", cwd, 42).is_err());
        assert!(record_plan("folders = 1", cwd, 42).is_err());
    }

    #[test]
    fn toml_folder_trust_persists_exact_folder_and_refuses_malformed_record() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("trusted_folders.toml");
        assert!(!trusted(&path, "/owned").unwrap());
        assert_eq!(grant(&path, "/owned"), PreTrustOutcome::Granted);
        assert!(trusted(&path, "/owned").unwrap());
        assert!(!trusted(&path, "/sibling").unwrap());
        assert_eq!(grant(&path, "/owned"), PreTrustOutcome::AlreadyTrusted);
        std::fs::write(&path, "malformed").unwrap();
        assert!(matches!(grant(&path, "/owned"), PreTrustOutcome::Failed(_)));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "malformed");
    }
}
