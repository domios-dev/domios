use crate::plan::{Plan, PlanStatus};
use std::io;
use std::path::{Path, PathBuf};

const BACKUP_DIR: &str = "migration-backups";

pub fn backup_dir(project_root: &Path, plan_id: &str) -> PathBuf {
    project_root
        .join(".codeterm")
        .join("plans")
        .join(BACKUP_DIR)
        .join(plan_id)
}

pub fn create_backup(project_root: &Path, plan_id: &str, source: &Path) -> io::Result<PathBuf> {
    let backup = backup_dir(project_root, plan_id);
    if backup.is_dir() {
        return Ok(backup);
    }
    let staging = backup.with_extension("staging");
    remove_dir_if_exists(&staging)?;
    copy_dir(source, &staging)?;
    if let Some(parent) = backup.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&staging, &backup)?;
    Ok(backup)
}

pub fn restore(project_root: &Path, plan_id: &str) -> io::Result<PathBuf> {
    let backup = backup_dir(project_root, plan_id);
    let legacy = backup.join("plan.json");
    let text = std::fs::read_to_string(&legacy)?;
    let plan: Plan = serde_json::from_str(&text)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if plan.id != plan_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("backup id {} does not match requested {plan_id}", plan.id),
        ));
    }
    let target = target_dir(project_root, &plan.status, plan_id);
    if target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("refusing to overwrite existing plan {}", target.display()),
        ));
    }
    let staging = target.with_extension("restore-staging");
    remove_dir_if_exists(&staging)?;
    copy_dir(&backup, &staging)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&staging, &target)?;
    Ok(target)
}

fn target_dir(project_root: &Path, status: &PlanStatus, plan_id: &str) -> PathBuf {
    project_root
        .join(".codeterm")
        .join("plans")
        .join(super::paths::plan_bucket(status))
        .join(plan_id)
}

fn copy_dir(source: &Path, target: &Path) -> io::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir(&source_path, &target_path)?;
        } else {
            std::fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn remove_dir_if_exists(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
