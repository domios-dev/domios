use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::references::path_key;
use crate::html_preview::{checked_path, extension};

pub struct PreviewPolicy {
    references: std::sync::Arc<HashSet<String>>,
    /// Items of a referenced `<ds-canvas src>` file count as referenced through that board.
    canvas_items: HashSet<String>,
    denied: Vec<String>,
    uploads: Vec<PathBuf>,
    upload_denied: Vec<String>,
    referenced_uploads: Vec<PathBuf>,
    referenced_upload_denied: Vec<String>,
    own_pad: Option<PathBuf>,
    exact_pad_read: Option<(PathBuf, PathBuf)>,
    agent_pads: Option<(PathBuf, PathBuf)>,
    pad_denied: Vec<String>,
    workspace: Option<PathBuf>,
    workspace_denied: Vec<String>,
}

pub struct PreviewRoots {
    pub configured: Vec<PathBuf>,
    pub private: Vec<PathBuf>,
    pub uploads: Vec<PathBuf>,
    pub own_pad: Option<PathBuf>,
    pub agent_pads: Option<PathBuf>,
    /// The tab's working directory; it may sit under a private root, such as a provisioned workspace.
    pub workspace: Option<PathBuf>,
    pub ssh: Option<PathBuf>,
}

impl PreviewPolicy {
    pub fn for_context(references: HashSet<String>, roots: PreviewRoots) -> Result<Self, String> {
        let mut private = roots.private;
        private.extend(roots.agent_pads.iter().cloned());
        let mut denied = roots.configured.clone();
        denied.extend(private.iter().cloned());
        denied.extend(roots.ssh.clone());
        let mut evidence_denied = roots.configured.clone();
        evidence_denied.extend(roots.ssh);
        let mut policy = Self::new(references, &denied)?;
        for uploads in roots.uploads {
            policy = policy.with_uploads(&uploads, &roots.configured)?;
        }
        if let Some(root) = &roots.agent_pads {
            let root = checked_path(&root.to_string_lossy())?;
            if let Ok(canonical) = root.canonicalize() {
                policy.agent_pads = Some((root, canonical));
            }
        }
        if let Some(pad) = roots.own_pad {
            policy = policy.with_own_pad(&pad, &evidence_denied)?;
        } else if policy.agent_pads.is_some() {
            policy.pad_denied = Self::new(HashSet::new(), &evidence_denied)?.denied;
        }
        if let Some(workspace) = roots.workspace {
            let mut workspace_denied = evidence_denied;
            workspace_denied.extend(roots.agent_pads);
            policy = policy.with_workspace(&workspace, &private, &workspace_denied)?;
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
            canvas_items: HashSet::new(),
            denied: roots,
            uploads: Vec::new(),
            upload_denied: Vec::new(),
            referenced_uploads: Vec::new(),
            referenced_upload_denied: Vec::new(),
            own_pad: None,
            exact_pad_read: None,
            agent_pads: None,
            pad_denied: Vec::new(),
            workspace: None,
            workspace_denied: Vec::new(),
        })
    }

    pub fn with_reference_inventory(mut self, references: std::sync::Arc<HashSet<String>>) -> Self {
        self.references = references;
        self
    }

    pub fn with_exact_pad_read(mut self, pad: PathBuf, file: PathBuf) -> Self {
        self.exact_pad_read = Some((pad, file));
        self
    }

    pub fn allows_pad_file(&self, pad: &Path, requested: &Path, resolved: &Path) -> bool {
        self.inside(pad, &self.pad_denied, requested, Some(resolved))
            && !self
                .pad_denied
                .iter()
                .any(|root| within(&denial_key(requested), root))
    }

    /// Admits the item paths of a canvas file this policy already authorized; every other rule,
    /// such as private roots and secret names, still applies to them.
    pub fn with_canvas_items(mut self, items: impl IntoIterator<Item = String>) -> Self {
        self.canvas_items.extend(
            items
                .into_iter()
                .filter_map(|item| checked_path(&item).ok())
                .map(|path| path_key(&path)),
        );
        self
    }

    fn referenced(&self, requested: &Path) -> bool {
        let key = path_key(requested);
        self.references.contains(&key) || self.canvas_items.contains(&key)
    }

    /// Owner write-back: a readable file outside the upload and attachment buckets, which hold
    /// pasted files and mesh-delivered copies rather than files the owner works on.
    pub fn authorize_write(
        &self,
        requested: &Path,
        canonical: Option<&Path>,
    ) -> Result<(), String> {
        if self.exact_pad_read.is_some() {
            return Err("Preview path is not writable".into());
        }
        self.authorize(requested, canonical)?;
        let buckets = self.uploads.iter().chain(&self.referenced_uploads);
        let resolved = canonical
            .map(Path::to_path_buf)
            .or_else(|| requested.canonicalize().ok());
        let inside = |path: &Path| buckets.clone().any(|root| path.starts_with(root));
        if std::iter::once(requested)
            .chain(resolved.as_deref())
            .any(inside)
        {
            return Err("Preview path is not writable".into());
        }
        Ok(())
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

    pub(crate) fn authorize_image(
        &self,
        requested: &Path,
        canonical: Option<&Path>,
    ) -> Result<(), String> {
        if self.exact_pad_read.is_some() {
            return self.authorize(requested, canonical);
        }
        if let Some((root, canonical_root)) = &self.agent_pads {
            let requested_key = denial_key(requested);
            if matches!(
                extension(requested).as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"
            ) && self.referenced(requested)
                && within(&requested_key, &denial_key(root))
                && !self
                    .pad_denied
                    .iter()
                    .any(|root| within(&requested_key, root))
            {
                let resolved = canonical
                    .map(Path::to_path_buf)
                    .or_else(|| requested.canonicalize().ok());
                if resolved.as_deref().is_some_and(|path| {
                    path.parent() != Some(canonical_root)
                        && self.inside(canonical_root, &self.pad_denied, requested, Some(path))
                }) {
                    return Ok(());
                }
            }
        }
        self.authorize(requested, canonical)
    }

    pub fn authorize(&self, requested: &Path, canonical: Option<&Path>) -> Result<(), String> {
        if let Some((pad, file)) = &self.exact_pad_read {
            let resolved = canonical
                .map(Path::to_path_buf)
                .or_else(|| requested.canonicalize().ok());
            return if path_key(requested) == path_key(file)
                && resolved.as_ref() == Some(file)
                && self.allows_pad_file(pad, requested, file)
            {
                Ok(())
            } else {
                Err("Preview path is private".into())
            };
        }
        if self.referenced(requested) {
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
        if self.referenced(requested) {
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
        if !self.referenced(requested) {
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
