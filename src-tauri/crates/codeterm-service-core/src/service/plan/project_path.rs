use crate::service::ServiceContext;
use codeterm_plan::service_plan::project_paths::PlanProjectUpdate;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProjectFolderState {
    Directory,
    Missing,
    NotDirectory,
    Unreadable(String),
}

fn validate_project_folder(
    path: &str,
    absolute: bool,
    state: ProjectFolderState,
) -> Result<(), String> {
    if path.trim().is_empty() || !absolute {
        return Err("Choose an existing absolute project folder".into());
    }
    match state {
        ProjectFolderState::Directory => Ok(()),
        ProjectFolderState::Missing => Err(format!("Project folder {path} does not exist")),
        ProjectFolderState::NotDirectory => Err(format!("Project path {path} is not a directory")),
        ProjectFolderState::Unreadable(error) => Err(format!(
            "Project folder {path} cannot be inspected: {error}"
        )),
    }
}

impl ServiceContext {
    pub fn set_plan_project(&self, plan_id: &str, path: &str) -> Result<PlanProjectUpdate, String> {
        self.ensure_storage_available()?;
        let folder = std::path::Path::new(path);
        let state = match std::fs::metadata(folder) {
            Ok(metadata) if metadata.is_dir() => ProjectFolderState::Directory,
            Ok(_) => ProjectFolderState::NotDirectory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ProjectFolderState::Missing
            }
            Err(error) => ProjectFolderState::Unreadable(error.to_string()),
        };
        validate_project_folder(path, folder.is_absolute(), state)?;
        let project = codeterm_paths::strip_verbatim_prefix(path);
        let canonical = crate::service::plan::paths::project_root(folder);
        let canonical = codeterm_paths::strip_verbatim_prefix(&canonical.display().to_string());
        let update = self
            .plan_store
            .set_project_paths(
                plan_id,
                &project,
                &canonical,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|error| error.to_string())?
                    .as_millis() as u64,
            )
            .map_err(|error| error.to_string())?;
        if update.changed {
            self.emit_plan_changed(&update.plan_id);
        }
        Ok(update)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_folder_validation_uses_inspection_evidence_on_every_platform() {
        for path in ["/replacement", "C:/Developer/codeterm"] {
            assert_eq!(
                validate_project_folder(path, true, ProjectFolderState::Directory),
                Ok(())
            );
            assert!(validate_project_folder(path, true, ProjectFolderState::Missing).is_err());
            assert!(validate_project_folder(path, true, ProjectFolderState::NotDirectory).is_err());
            assert!(validate_project_folder(
                path,
                true,
                ProjectFolderState::Unreadable("denied".into())
            )
            .is_err());
        }
        assert!(validate_project_folder("", true, ProjectFolderState::Directory).is_err());
        assert!(validate_project_folder("relative", false, ProjectFolderState::Directory).is_err());
    }
}
