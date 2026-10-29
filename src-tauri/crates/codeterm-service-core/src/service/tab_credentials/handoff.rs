use std::path::{Path, PathBuf};

pub(crate) struct CredentialFile(pub(crate) PathBuf);

impl CredentialFile {
    pub(crate) fn path(&self) -> &Path { &self.0 }
}

impl Drop for CredentialFile {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.0) {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::debug!("launch credential file cleanup failed: {error}");
            }
        }
    }
}

pub(super) struct CredentialHandoff {
    pub(super) nonce: String,
    pub(super) pty_id: String,
    pub(super) _file: CredentialFile,
}
