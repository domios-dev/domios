use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::references::path_key;
use crate::html_preview::checked_path;

pub struct PreviewPolicy {
    references: HashSet<String>,
    denied: Vec<String>,
    uploads: Vec<PathBuf>,
    upload_denied: Vec<String>,
}

impl PreviewPolicy {
    pub fn new(references: HashSet<String>, denied: &[PathBuf]) -> Result<Self, String> {
        let mut roots = Vec::new();
        for path in denied {
            let validated = checked_path(&path.to_string_lossy())?;
            roots.push(denial_key(&validated));
            if let Ok(canonical) = validated.canonicalize() {
                roots.push(denial_key(&canonical));
            }
        }
        Ok(Self {
            references,
            denied: roots,
            uploads: Vec::new(),
            upload_denied: Vec::new(),
        })
    }

    pub fn with_uploads(mut self, uploads: &Path, denied: &[PathBuf]) -> Result<Self, String> {
        self.upload_denied = Self::new(HashSet::new(), denied)?.denied;
        if let Ok(root) = uploads.canonicalize() {
            self.uploads.push(root);
        }
        Ok(self)
    }

    pub fn authorize(&self, requested: &Path, canonical: Option<&Path>) -> Result<(), String> {
        // Uploads are user-provided artifacts; require a canonical direct child to exclude symlink escapes.
        for uploads in &self.uploads {
            if requested
                .parent()
                .and_then(|parent| parent.canonicalize().ok())
                .as_ref()
                == Some(uploads)
                && canonical.is_none_or(|path| path.parent() == Some(uploads.as_path()))
                && !self.upload_denied.iter().any(|root| {
                    std::iter::once(requested).chain(canonical).any(|path| {
                        let key = denial_key(path);
                        key == *root
                            || key
                                .strip_prefix(root.as_str())
                                .is_some_and(|rest| rest.starts_with('/'))
                    })
                })
            {
                return Ok(());
            }
        }
        if !self.references.contains(&path_key(requested)) {
            return Err("Preview path is not referenced".into());
        }
        for path in std::iter::once(requested).chain(canonical) {
            let key = denial_key(path);
            let denied_root = self.denied.iter().any(|root| {
                key == *root
                    || key
                        .strip_prefix(root.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            });
            if denied_root || key.split('/').any(secret_component) {
                return Err("Preview path is private".into());
            }
        }
        Ok(())
    }
}

fn denial_key(path: &Path) -> String {
    let key = path_key(path);
    if cfg!(windows) {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

fn secret_component(component: &str) -> bool {
    let name = component.to_ascii_lowercase();
    if name.starts_with(".codeterm")
        || name == ".env"
        || name.starts_with(".env.")
        || matches!(
            name.as_str(),
            ".ssh"
                | ".aws"
                | ".azure"
                | ".gnupg"
                | ".kube"
                | ".docker"
                | ".codex"
                | ".claude"
                | ".netrc"
                | ".npmrc"
                | ".pypirc"
        )
        || name.contains("token")
        || name.contains("credential")
        || name.contains("password")
        || matches!(
            name.as_str(),
            "auth.json" | "oauth.json" | "authentication.json"
        )
        || name.starts_with("id_rsa")
        || name.starts_with("id_dsa")
        || name.starts_with("id_ecdsa")
        || name.starts_with("id_ed25519")
        || name.ends_with(".pem")
        || name.ends_with(".key")
    {
        return true;
    }
    name.split(['.', '-', '_']).any(|part| {
        matches!(
            part,
            "secret"
                | "secrets"
                | "credential"
                | "credentials"
                | "token"
                | "tokens"
                | "password"
                | "passwords"
        )
    })
}
