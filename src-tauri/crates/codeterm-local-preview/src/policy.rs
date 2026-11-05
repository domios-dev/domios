use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::references::path_key;
use crate::html_preview::checked_path;

pub struct PreviewPolicy {
    references: std::sync::Arc<HashSet<String>>,
    denied: Vec<String>,
    uploads: Vec<PathBuf>,
    upload_denied: Vec<String>,
    referenced_uploads: Vec<PathBuf>,
    referenced_upload_denied: Vec<String>,
    own_pad: Option<PathBuf>,
    pad_denied: Vec<String>,
    workspace: Option<PathBuf>,
    workspace_denied: Vec<String>,
}

pub struct PreviewRoots {
    pub configured: Vec<PathBuf>,
    pub private: Vec<PathBuf>,
    pub uploads: Vec<PathBuf>,
    pub own_pad: Option<PathBuf>,
    /// The tab's working directory; it may sit under a private root, such as a provisioned workspace.
    pub workspace: Option<PathBuf>,
    pub ssh: Option<PathBuf>,
}

impl PreviewPolicy {
    pub fn for_context(references: HashSet<String>, roots: PreviewRoots) -> Result<Self, String> {
        let private = roots.private.clone();
        let mut denied = roots.configured.clone();
        denied.extend(roots.private);
        denied.extend(roots.ssh.clone());
        let mut evidence_denied = roots.configured.clone();
        evidence_denied.extend(roots.ssh);
        let mut policy = Self::new(references, &denied)?;
        for uploads in roots.uploads {
            policy = policy.with_uploads(&uploads, &roots.configured)?;
        }
        if let Some(pad) = roots.own_pad {
            policy = policy.with_own_pad(&pad, &evidence_denied)?;
        }
        if let Some(workspace) = roots.workspace {
            policy = policy.with_workspace(&workspace, &private, &evidence_denied)?;
        }
        Ok(policy)
    }

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
            references: references.into(),
            denied: roots,
            uploads: Vec::new(),
            upload_denied: Vec::new(),
            referenced_uploads: Vec::new(),
            referenced_upload_denied: Vec::new(),
            own_pad: None,
            pad_denied: Vec::new(),
            workspace: None,
            workspace_denied: Vec::new(),
        })
    }

    pub fn with_reference_inventory(mut self, references: std::sync::Arc<HashSet<String>>) -> Self {
        self.references = references;
        self
    }

    pub fn with_uploads(mut self, uploads: &Path, denied: &[PathBuf]) -> Result<Self, String> {
        self.upload_denied = Self::new(HashSet::new(), denied)?.denied;
        if let Ok(root) = uploads.canonicalize() {
            self.uploads.push(root);
        }
        Ok(self)
    }

    /// Allow referenced files directly inside a trusted evidence directory while keeping other
    /// paths under denied roots private.
    pub fn with_referenced_uploads(
        mut self,
        uploads: &Path,
        denied: &[PathBuf],
    ) -> Result<Self, String> {
        self.referenced_upload_denied = Self::new(HashSet::new(), denied)?.denied;
        if let Ok(root) = uploads.canonicalize() {
            self.referenced_uploads.push(root);
        }
        Ok(self)
    }

    pub fn with_own_pad(mut self, root: &Path, denied: &[PathBuf]) -> Result<Self, String> {
        let canonical = root
            .canonicalize()
            .map_err(|_| "Unable to resolve preview pad")?;
        self.own_pad = Some(canonical);
        self.pad_denied = Self::new(HashSet::new(), denied)?.denied;
        Ok(self)
    }

    /// Admit referenced files under the tab's workspace even when it lives under a private root.
    /// A workspace that contains a private root gets no grant, and private roots that are not its
    /// ancestors stay refused inside it.
    pub fn with_workspace(
        mut self,
        root: &Path,
        private: &[PathBuf],
        denied: &[PathBuf],
    ) -> Result<Self, String> {
        let Ok(canonical) = root.canonicalize() else {
            return Ok(self);
        };
        let key = denial_key(&canonical);
        let private = Self::new(HashSet::new(), private)?.denied;
        if private.iter().any(|root| within(root, &key)) {
            return Ok(self);
        }
        let mut workspace_denied = Self::new(HashSet::new(), denied)?.denied;
        workspace_denied.extend(private.into_iter().filter(|root| !within(&key, root)));
        self.workspace = Some(canonical);
        self.workspace_denied = workspace_denied;
        Ok(self)
    }

    fn inside(
        &self,
        root: &Path,
        denied: &[String],
        requested: &Path,
        canonical: Option<&Path>,
    ) -> bool {
        let resolved = canonical
            .map(Path::to_path_buf)
            .or_else(|| requested.canonicalize().ok());
        resolved.as_deref().is_some_and(|path| {
            let Ok(relative) = path.strip_prefix(root) else {
                return false;
            };
            !relative.as_os_str().is_empty()
                && !relative
                    .components()
                    .any(|part| secret_component(&part.as_os_str().to_string_lossy()))
                && !denied
                    .iter()
                    .any(|denied| within(&denial_key(path), denied))
        })
    }

    pub fn authorize(&self, requested: &Path, canonical: Option<&Path>) -> Result<(), String> {
        if self.references.contains(&path_key(requested)) {
            let scoped = [
                (&self.own_pad, &self.pad_denied),
                (&self.workspace, &self.workspace_denied),
            ];
            for (root, denied) in scoped {
                if root
                    .as_deref()
                    .is_some_and(|root| self.inside(root, denied, requested, canonical))
                {
                    return Ok(());
                }
            }
        }
        if self.references.contains(&path_key(requested)) {
            for uploads in &self.referenced_uploads {
                let direct_child = requested
                    .parent()
                    .and_then(|parent| parent.canonicalize().ok())
                    .as_ref()
                    == Some(uploads)
                    && canonical.is_none_or(|path| path.parent() == Some(uploads.as_path()));
                let denied = self.referenced_upload_denied.iter().any(|root| {
                    std::iter::once(requested).chain(canonical).any(|path| {
                        let key = denial_key(path);
                        key == *root
                            || key
                                .strip_prefix(root.as_str())
                                .is_some_and(|rest| rest.starts_with('/'))
                    })
                });
                let secret = std::iter::once(requested)
                    .chain(canonical)
                    .any(|path| path_key(path).split('/').any(secret_component));
                if direct_child && !denied && !secret {
                    return Ok(());
                }
            }
        }
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

fn within(key: &str, root: &str) -> bool {
    key == root
        || key
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
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
    // Tracked legacy directories stay private until the owner moves them.
    if name.starts_with(".domios")
        || name.starts_with(".codeterm")
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
