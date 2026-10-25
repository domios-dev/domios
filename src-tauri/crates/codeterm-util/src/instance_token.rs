use std::path::Path;

use crate::secret_file::{SecretFile, SecretFileError};

const SERVICE: &str = "domios.instance";
const ACCOUNT: &str = "api_token";

pub fn read_at(data_root: &Path) -> Result<Option<String>, String> {
    let path = data_root.join(".secrets");
    if std::fs::metadata(&path).is_ok_and(|metadata| !metadata.is_file()) {
        return Err(format!(
            "Instance credential path {} must resolve to a regular file",
            path.display()
        ));
    }
    match store(data_root)?.get(ACCOUNT) {
        Ok(token) if !token.trim().is_empty() => Ok(Some(token)),
        Ok(_) => Err("Instance API credential is empty".into()),
        Err(SecretFileError::NotFound) => Ok(None),
        Err(_) => Err("Cannot read instance API credential from .secrets".into()),
    }
}

pub fn initialize_at(
    data_root: &Path,
    legacy_token: Option<&str>,
    generate: bool,
) -> Result<Option<String>, String> {
    if let Some(token) = read_at(data_root)? {
        return Ok(Some(token));
    }
    let token = legacy_token
        .filter(|token| !token.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| generate.then(|| uuid::Uuid::new_v4().simple().to_string()));
    if let Some(token) = &token {
        store(data_root)?
            .set(ACCOUNT, token)
            .map_err(|_| "Cannot persist instance API credential in .secrets".to_string())?;
    }
    Ok(token)
}

/// Rotate legacy authority before its config entry or recovery copies lose their role.
pub fn migrate_at(data_root: &Path, legacy_token: &str) -> Result<String, String> {
    if let Some(token) = read_at(data_root)? {
        if token != legacy_token {
            return Ok(token);
        }
    }
    let token = uuid::Uuid::new_v4().simple().to_string();
    store(data_root)?
        .set(ACCOUNT, &token)
        .map_err(|_| "Cannot persist rotated instance API credential in .secrets".to_string())?;
    Ok(token)
}

pub fn resolve_at(
    data_root: &Path,
    environment: Option<&str>,
    require_stored: bool,
) -> Result<Option<String>, String> {
    if !require_stored {
        if let Some(token) = environment.filter(|token| !token.trim().is_empty()) {
            return Ok(Some(token.to_owned()));
        }
    }
    read_at(data_root)
}

fn store(data_root: &Path) -> Result<SecretFile, String> {
    let store = SecretFile::new_at(data_root, SERVICE);
    if store.is_available() {
        Ok(store)
    } else {
        Err("Protected instance secret store is unavailable".into())
    }
}

#[cfg(test)]
mod tests;
