use super::*;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Entry {
    pub file_id: String,
    pub message_id: String,
    pub name: String,
    pub mime: String,
    pub size_bytes: usize,
    pub direction: String,
    pub state: String,
}

impl Entry {
    pub(super) fn new(file: &Manifest, envelope: &Envelope, direction: &str, state: &str) -> Self {
        Self { file_id: file.file_id.clone(), message_id: envelope.message_id.clone(), name: files::safe_name(&file.name), mime: file.mime.clone(), size_bytes: file.plaintext_size, direction: direction.into(), state: state.into() }
    }

    pub(in crate::service::neighbors::runtime::files) fn filename(&self) -> String {
        let mut name = files::safe_name(&self.name);
        while name.len() > 200 { name.pop(); }
        format!("{}-{name}", self.file_id)
    }
}

#[derive(Default)]
pub(super) struct Cell {
    pub writes: tokio::sync::Mutex<()>,
    pub transfers: parking_lot::Mutex<HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>,
}

impl ServiceContext {
    pub(super) fn file_cell(&self) -> &Cell { self.extensions.get_or_init(Cell::default) }

    pub(super) async fn record_neighbor_file(&self, metadata: &Metadata, friendship: &str, entry: Entry) -> codeterm_neighbors::Result<()> {
        let _write = self.file_cell().writes.lock().await;
        let mut entries = metadata.file_entries(friendship).await?;
        if let Some(old) = entries.iter_mut().find(|old| old.file_id == entry.file_id && old.direction == entry.direction) { *old = entry; }
        else { entries.push(entry); }
        let excess = entries.len().saturating_sub(256);
        entries.drain(..excess);
        metadata.write(format!("files-{friendship}.json"), entries).await?;
        self.ui.emit("neighbor-files-changed", json!({"friendship_id":friendship}));
        Ok(())
    }
}

impl Metadata {
    pub(super) async fn file_entries(&self, friendship: &str) -> codeterm_neighbors::Result<Vec<Entry>> {
        if !hex_id(friendship, 64) { return Err(Error::InvalidRequest); }
        Ok(self.read(format!("files-{friendship}.json")).await?.unwrap_or_default())
    }

    pub(super) async fn saved_file(&self, friendship: &str, file_id: &str) -> codeterm_neighbors::Result<PathBuf> {
        if !hex_id(file_id, 32) { return Err(Error::InvalidRequest); }
        let entry = self.file_entries(friendship).await?.into_iter()
            .find(|entry| entry.file_id == file_id && entry.state == "saved" && entry.direction == "in")
            .ok_or(Error::InvalidRequest)?;
        let folder = self.friend_folder(friendship)?;
        tokio::task::spawn_blocking(move || {
            let folder = folder.canonicalize().map_err(|_| Error::CursorStore)?;
            let path = folder.join(entry.filename());
            let metadata = std::fs::symlink_metadata(&path).map_err(|_| Error::CursorStore)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() || path.canonicalize().map_err(|_| Error::CursorStore)?.parent() != Some(folder.as_path()) {
                return Err(Error::InvalidRequest);
            }
            Ok(path)
        }).await.map_err(|_| Error::CursorStore)?
    }
}

pub(super) async fn save(folder: PathBuf, entry: Entry, plaintext: Zeroizing<Vec<u8>>) -> codeterm_neighbors::Result<()> {
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        codeterm_util::private_file::create_dir_all(&folder).map_err(|_| Error::CursorStore)?;
        let folder = folder.canonicalize().map_err(|_| Error::CursorStore)?;
        let path = folder.join(entry.filename());
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(|_| Error::CursorStore)?;
        if file.write_all(&plaintext).and_then(|()| file.sync_all()).is_err() {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(Error::CursorStore);
        }
        #[cfg(unix)]
        std::fs::File::open(&folder).and_then(|directory| directory.sync_all()).map_err(|_| Error::CursorStore)?;
        Ok(())
    }).await.map_err(|_| Error::CursorStore)?
}

pub(super) async fn open_folder(path: PathBuf) -> codeterm_neighbors::Result<()> {
    let folder = path.clone();
    tokio::task::spawn_blocking(move || codeterm_util::private_file::create_dir_all(&folder))
        .await.map_err(|_| Error::CursorStore)?.map_err(|_| Error::CursorStore)?;
    open(path).await
}

pub(super) async fn open(path: PathBuf) -> codeterm_neighbors::Result<()> {
    tokio::task::spawn_blocking(move || {
        if !path.exists() { return Err(Error::InvalidRequest); }
        let program = if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let mut command = std::process::Command::new(program);
        command.arg(&path);
        #[cfg(windows)]
        codeterm_util::process_util::no_window_std(&mut command);
        command.spawn().map_err(|_| Error::Gateway)?;
        Ok(())
    }).await.map_err(|_| Error::Gateway)?
}
