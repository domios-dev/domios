use super::ports::AccountMeshStore;
use super::state::AccountMeshRecord;
use async_trait::async_trait;
use std::{fs, io, path::PathBuf};

pub struct FileAccountMeshStore {
    path: PathBuf,
}

impl FileAccountMeshStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            path: data_dir.join("state/account/mesh.json"),
        }
    }
}

fn read(path: &PathBuf) -> io::Result<AccountMeshRecord> {
    let mut record: AccountMeshRecord = match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => AccountMeshRecord::default(),
        Err(error) => return Err(error),
    };
    record.admission_cache = fs::read(path.with_file_name("mesh-admission.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    record.reauth_required_accounts = fs::read(path.with_file_name("mesh-reauth.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    match fs::read(path.with_file_name("mesh-peer-presence.json")) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(presence) => record.peer_presence = presence,
            Err(_) => record.peer_presence_refresh = record.owner.is_some(),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => record.peer_presence_refresh = record.owner.is_some(),
    }
    Ok(record)
}

fn write(path: &PathBuf, record: &AccountMeshRecord) -> io::Result<()> {
    read(path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let reauth_path = path.with_file_name("mesh-reauth.json");
    let temporary_reauth = reauth_path.with_extension("json.tmp");
    fs::write(
        &temporary_reauth,
        serde_json::to_vec(&record.reauth_required_accounts).map_err(io::Error::other)?,
    )?;
    fs::rename(&temporary_reauth, &reauth_path)?;
    if !record.peer_presence_refresh {
        let presence_path = path.with_file_name("mesh-peer-presence.json");
        let temporary_presence = presence_path.with_extension("json.tmp");
        fs::write(
            &temporary_presence,
            serde_json::to_vec(&record.peer_presence).map_err(io::Error::other)?,
        )?;
        fs::rename(&temporary_presence, &presence_path)?;
    }
    let bytes = serde_json::to_vec(record).map_err(io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)?;
    let cache_path = path.with_file_name("mesh-admission.json");
    match &record.admission_cache {
        Some(cache) => {
            let temporary = cache_path.with_extension("json.tmp");
            fs::write(
                &temporary,
                serde_json::to_vec(cache).map_err(io::Error::other)?,
            )?;
            fs::rename(temporary, cache_path)
        }
        None => match fs::remove_file(cache_path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        },
    }
}

#[async_trait]
impl AccountMeshStore for FileAccountMeshStore {
    async fn load(&self) -> io::Result<AccountMeshRecord> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || read(&path))
            .await
            .map_err(io::Error::other)?
    }
    async fn save(&self, record: &AccountMeshRecord) -> io::Result<()> {
        let path = self.path.clone();
        let record = record.clone();
        tokio::task::spawn_blocking(move || write(&path, &record))
            .await
            .map_err(io::Error::other)?
    }
}

#[cfg(test)]
mod tests;
