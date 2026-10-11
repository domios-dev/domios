use std::path::{Path, PathBuf};

use super::PluginSandbox;

impl PluginSandbox {
    fn path_under(roots: &[PathBuf], path: &str) -> bool {
        let Some(target) = codeterm_util::project_roots::canonical_target(Path::new(path)) else {
            return false;
        };
        roots.iter().any(|root| {
            std::fs::canonicalize(root)
                .map(|r| target.starts_with(&r))
                .unwrap_or(false)
        })
    }

    fn project_origin(&self, path: &str) -> Option<&PathBuf> {
        Path::new(path)
            .ancestors()
            .filter_map(|ancestor| {
                let canonical = std::fs::canonicalize(ancestor).ok()?;
                self.project_read_roots
                    .iter()
                    .find(|root| **root == canonical)
            })
            .last()
    }

    pub(in crate::js_runtime) fn with_project_roots(
        mut self,
        roots: Vec<PathBuf>,
        granted: Vec<PathBuf>,
        grants_path: PathBuf,
    ) -> Self {
        self.project_write_roots = roots
            .iter()
            .filter(|root| granted.contains(root))
            .cloned()
            .collect();
        self.project_read_roots = roots;
        self.denied_writes.push(grants_path);
        self
    }

    pub(in crate::js_runtime) fn can_read(&self, path: &str) -> bool {
        if self.is_denied_read(path) {
            self.record_denial();
            return false;
        }
        let allowed = if let Some(root) = self.project_origin(path) {
            Self::path_under(std::slice::from_ref(root), path)
        } else {
            self.unrestricted
                || Self::path_under(&self.read_roots, path)
                || Self::path_under(&self.project_read_roots, path)
                || Self::path_under(&self.granted_read_roots.lock(), path)
        };
        if !allowed {
            self.record_denial();
        }
        allowed
    }

    pub(in crate::js_runtime) fn can_write(&self, path: &str) -> bool {
        let target = codeterm_util::project_roots::canonical_target(Path::new(path));
        let denied = target.as_ref().is_some_and(|target| {
            self.denied_writes.iter().any(|denied| {
                codeterm_util::project_roots::canonical_target(denied).as_ref() == Some(target)
            })
        });
        let allowed = !denied
            && if let Some(root) = self.project_origin(path) {
                self.project_write_roots.contains(root)
                    && Self::path_under(std::slice::from_ref(root), path)
            } else if Self::path_under(&self.project_read_roots, path) {
                Self::path_under(&self.project_write_roots, path)
            } else {
                self.unrestricted || Self::path_under(&self.write_roots, path)
            };
        if !allowed {
            self.record_denial();
        }
        allowed
    }

    pub(in crate::js_runtime) fn project_write_allowed(&self, root: &str) -> bool {
        codeterm_util::project_roots::canonical_root(root)
            .ok()
            .is_some_and(|root| self.project_write_roots.contains(&root))
    }
}
