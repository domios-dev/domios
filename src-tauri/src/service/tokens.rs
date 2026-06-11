use rand::RngCore;
use std::fs;
use std::io::Write;
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
        fs::create_dir_all(path.parent().unwrap())?;
        let mut f = fs::File::create(&path)?;
        f.write_all(token.as_bytes())?;
        drop(f);
        restrict_file_permissions(&path);
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

fn restrict_file_permissions(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            log::warn!(
                "[services/tokens] failed to restrict token permissions {}: {}",
                path.display(),
                e
            );
        }
    }
    #[cfg(windows)]
    {
        let path_str = path.to_string_lossy();
        let username = std::env::var("USERNAME").unwrap_or_default();
        if !username.is_empty() {
            if let Err(e) = std::process::Command::new("icacls")
                .args([
                    path_str.as_ref(),
                    "/inheritance:r",
                    "/grant",
                    &format!("{}:(F)", username),
                ])
                .output()
            {
                log::warn!(
                    "[services/tokens] failed to restrict token ACL {}: {}",
                    path.display(),
                    e
                );
            }
        }
    }
}

pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn generate_persist_validate_round_trip() {
        let dir = tempdir().unwrap();
        let store = TokenStore::new_with_root(dir.path().to_path_buf());
        let token = store.generate("svc_test").unwrap();
        assert_eq!(token.len(), 64);
        assert!(store.validate("svc_test", &token).unwrap());
        assert!(!store.validate("svc_test", "wrong-token").unwrap());
    }

    #[test]
    fn missing_service_returns_none() {
        let dir = tempdir().unwrap();
        let store = TokenStore::new_with_root(dir.path().to_path_buf());
        assert!(!store.validate("svc_nonexistent", "any").unwrap());
    }

    #[test]
    fn rotate_invalidates_old_token() {
        let dir = tempdir().unwrap();
        let store = TokenStore::new_with_root(dir.path().to_path_buf());
        let old = store.generate("svc_x").unwrap();
        let new = store.generate("svc_x").unwrap();
        assert_ne!(old, new);
        assert!(!store.validate("svc_x", &old).unwrap());
        assert!(store.validate("svc_x", &new).unwrap());
    }

    #[test]
    fn lookup_service_id_by_token() {
        let dir = tempdir().unwrap();
        let store = TokenStore::new_with_root(dir.path().to_path_buf());
        let token_a = store.generate("svc_a").unwrap();
        let token_b = store.generate("svc_b").unwrap();
        assert_eq!(
            store.lookup_service_id(&token_a).unwrap(),
            Some("svc_a".to_string())
        );
        assert_eq!(
            store.lookup_service_id(&token_b).unwrap(),
            Some("svc_b".to_string())
        );
        assert_eq!(store.lookup_service_id("garbage").unwrap(), None);
    }

    #[test]
    fn delete_removes_token_dir() {
        let dir = tempdir().unwrap();
        let store = TokenStore::new_with_root(dir.path().to_path_buf());
        let _ = store.generate("svc_to_delete").unwrap();
        store.delete("svc_to_delete").unwrap();
        assert!(!store.validate("svc_to_delete", "any").unwrap());
    }
}
