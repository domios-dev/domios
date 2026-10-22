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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[tokio::test]
    async fn unreadable_mesh_file_is_preserved_and_cannot_be_overwritten() {
        let directory = std::env::temp_dir().join(format!(
            "codeterm-mesh-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("mesh.json");
        let corrupt = b"{not-json";
        fs::write(&path, corrupt).unwrap();
        let store = FileAccountMeshStore { path: path.clone() };

        assert!(store.load().await.is_err());
        assert!(store.save(&AccountMeshRecord::default()).await.is_err());
        assert_eq!(fs::read(&path).unwrap(), corrupt);

        fs::remove_dir_all(directory).unwrap();
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
