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
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AccountMeshRecord::default()),
        Err(error) => Err(error),
    }
}

fn write(path: &PathBuf, record: &AccountMeshRecord) -> io::Result<()> {
    read(path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(record).map_err(io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
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
