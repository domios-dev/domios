use rand::RngCore;
use std::fs;
use std::path::PathBuf;

pub struct TokenStore {
    root: PathBuf,
}

impl TokenStore {
    pub fn new() -> Self {
        let root = crate::paths::data_dir().join("services");
        Self::new_with_root(root)
    }

    pub fn new_with_root(root: PathBuf) -> Self {
        if let Err(e) = fs::create_dir_all(&root) {
            log::warn!(
                "[services/tokens] failed to create token root {}: {}",
                root.display(),
                e
            );
        }
        Self { root }
    }

    fn token_path(&self, service_id: &str) -> PathBuf {
        self.root.join(service_id).join("token")
    }

    pub fn generate(&self, service_id: &str) -> std::io::Result<String> {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = bytes
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        let path = self.token_path(service_id);
        codeterm_util::private_file::write_atomic(&path, token.as_bytes())?;
        Ok(token)
    }

    pub fn validate(&self, service_id: &str, candidate: &str) -> std::io::Result<bool> {
        let path = self.token_path(service_id);
        if !path.exists() {
            return Ok(false);
        }
        let stored = fs::read_to_string(&path)?;
        Ok(constant_time_eq(
            stored.trim().as_bytes(),
            candidate.as_bytes(),
        ))
    }

    pub fn lookup_service_id(&self, token: &str) -> std::io::Result<Option<String>> {
        if !self.root.exists() {
            return Ok(None);
        }
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().to_string();
            if self.validate(&id, token)? {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    pub fn delete(&self, service_id: &str) -> std::io::Result<()> {
        let dir = self.root.join(service_id);
        if dir.exists() {
            fs::remove_dir_all(dir)?;
        }
        Ok(())
    }
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Non-leaking token fingerprint for diagnostics: byte length + a short
/// non-cryptographic hash. Never logs the token itself.
pub fn token_fp(token: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    token.hash(&mut h);
    format!("len={} fp={:08x}", token.len(), (h.finish() as u32))
}

#[cfg(test)]
mod tests;
