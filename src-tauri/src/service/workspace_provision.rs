use std::path::{Path, PathBuf};

use super::ServiceContext;
use crate::multiplexer::state::WorkspaceInfo;
use crate::service::session_persist::{self, SessionPersistUrgency};

const PROVISIONED_WORKSPACES_DIR: &str = "provisioned-workspaces";
const SLUG_MAX_CHARS: usize = 40;

fn directory_slug(name: &str) -> String {
    let slug: String = name
        .chars()
        .take(SLUG_MAX_CHARS)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "workspace".to_string()
    } else {
        slug.to_string()
    }
}

/// `create_dir` fails on any existing entry, so an existing path is refused, never adopted.
pub(crate) fn create_new_directory(path: &Path) -> Result<(), String> {
    match std::fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(format!(
            "refusing to provision {}: the path already exists",
            path.display()
        )),
        Err(error) => Err(format!(
            "create provisioned directory {}: {error}",
            path.display()
        )),
    }
}

pub(crate) fn provision_workspace_directory(root: &Path, name: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(root).map_err(|error| format!("create {}: {error}", root.display()))?;
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let path = root.join(format!("{}-{}", directory_slug(name), &unique[..12]));
    create_new_directory(&path)?;
    Ok(path)
}

impl ServiceContext {
    pub fn create_provisioned_workspace(
        &self,
        name: String,
        activate: bool,
    ) -> Result<WorkspaceInfo, String> {
        super::workspace::validate_workspace_name(&name)?;
        let root = crate::paths::data_dir().join(PROVISIONED_WORKSPACES_DIR);
        let dir = provision_workspace_directory(&root, &name)?;
        let cwd = dir.to_string_lossy().to_string();
        let ws = self
            .mux
            .write()
            .create_provisioned_workspace(name, cwd.clone());
        if activate {
            let _ = self.mux.write().set_active_workspace(&ws.id);
        }
        log::info!(
            "[workspace] provisioned '{}' ({}) at {}",
            ws.name,
            ws.id,
            cwd
        );
        self.emit_state_changed();
        if let Err(error) =
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Prompt)
        {
            log::warn!(
                "[workspace] persisting provisioned workspace {} failed: {error}",
                ws.id
            );
        }
        Ok(ws)
    }
}

#[cfg(test)]
mod tests {
    use super::{create_new_directory, directory_slug, provision_workspace_directory};

    struct TempRoot(std::path::PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "ct-provision-test-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&path).expect("temp root");
            Self(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn provision_creates_a_new_empty_directory_under_the_root() {
        let root = TempRoot::new();
        let workspaces = root.0.join("provisioned-workspaces");

        let first = provision_workspace_directory(&workspaces, "Restore Lifecycle").unwrap();
        let second = provision_workspace_directory(&workspaces, "Restore Lifecycle").unwrap();

        assert_ne!(first, second);
        for dir in [&first, &second] {
            assert!(dir.is_dir());
            assert_eq!(dir.parent(), Some(workspaces.as_path()));
            assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0);
        }
    }

    #[test]
    fn provision_refuses_an_existing_directory_without_adopting_it() {
        let root = TempRoot::new();
        let existing = root.0.join("user-project");
        std::fs::create_dir(&existing).unwrap();
        let marker = existing.join("marker.txt");
        std::fs::write(&marker, "user content").unwrap();

        assert!(create_new_directory(&existing).is_err());
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "user content");
    }

    #[test]
    fn provision_refuses_an_existing_file() {
        let root = TempRoot::new();
        let file = root.0.join("occupied");
        std::fs::write(&file, "x").unwrap();

        assert!(create_new_directory(&file).is_err());
        assert!(file.is_file());
    }

    #[test]
    fn directory_slug_never_carries_path_separators() {
        assert_eq!(directory_slug("../../etc"), "etc");
        assert_eq!(directory_slug("a\\b/c"), "a-b-c");
        assert_eq!(directory_slug("///"), "workspace");
    }
}
