use std::path::{Path, PathBuf};

use super::{paths, PlanError, PlanStore};
use crate::plan::{AttachmentRef, Plan};

pub const MAX_ATTACHMENTS_PER_RECORD: usize = 12;
pub const MAX_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub enum AttachmentTarget {
    Task,
    Section,
    Milestone,
}

#[derive(Debug, Clone)]
pub struct OrphanAttachmentDir {
    pub plan_id: String,
    pub path: PathBuf,
}

fn attachment_dir(plan_root: &Path, target: AttachmentTarget, entity_id: &str) -> PathBuf {
    plan_root
        .join("attachments")
        .join(target_dir(target))
        .join(entity_id)
}

pub fn safe_filename(path: &Path) -> String {
    let raw = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("attachment.bin");
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => c,
            _ => '_',
        })
        .collect();
    let trimmed = cleaned.trim_matches('.');
    if trimmed.is_empty() {
        "attachment.bin".into()
    } else {
        trimmed.chars().take(120).collect()
    }
}

fn target_dir(target: AttachmentTarget) -> &'static str {
    match target {
        AttachmentTarget::Task => "tasks",
        AttachmentTarget::Section => "sections",
        AttachmentTarget::Milestone => "milestones",
    }
}

fn media_type_for(path: &Path) -> String {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("txt" | "md") => "text/plain",
        _ => "application/octet-stream",
    }
    .into()
}

fn copy_attachment(
    plan_root: &Path,
    target: AttachmentTarget,
    entity_id: &str,
    source: &Path,
    now_ms: u64,
) -> Result<AttachmentRef, PlanError> {
    let meta = std::fs::metadata(source).map_err(|e| PlanError::Io(e.to_string()))?;
    if !meta.is_file() {
        return Err(PlanError::Io(format!(
            "attachment source is not a file: {}",
            source.display()
        )));
    }
    if meta.len() > MAX_ATTACHMENT_BYTES {
        return Err(PlanError::Io(format!(
            "attachment exceeds {} bytes: {}",
            MAX_ATTACHMENT_BYTES,
            source.display()
        )));
    }
    let id = crate::plan::ids::short_id("att");
    let safe = safe_filename(source);
    let dir = attachment_dir(plan_root, target, entity_id);
    std::fs::create_dir_all(&dir).map_err(|e| PlanError::Io(e.to_string()))?;
    let filename = format!("{id}-{safe}");
    let dest = dir.join(&filename);
    let size = codeterm_util::attachments::copy_complete_file(source, &dest, MAX_ATTACHMENT_BYTES)
        .map_err(PlanError::Io)?;
    let rel = dest
        .strip_prefix(plan_root)
        .map_err(|e| PlanError::Io(e.to_string()))?
        .to_string_lossy()
        .replace('\\', "/");
    Ok(AttachmentRef {
        id,
        name: safe,
        path: rel,
        media_type: media_type_for(source),
        size_bytes: size,
        created_at: now_ms,
    })
}

pub(super) fn remove_dir_if_exists(path: PathBuf) -> Result<(), PlanError> {
    match std::fs::remove_dir_all(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(PlanError::Io(e.to_string())),
    }
}

impl PlanStore {
    pub(super) fn plan_data_dir(&self, plan_id: &str) -> PathBuf {
        self.database
            .path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("plans")
            .join(plan_id)
    }

    pub fn attachment_abs_path(&self, plan: &Plan, rel_path: &str) -> Option<PathBuf> {
        if rel_path.contains("..") || Path::new(rel_path).is_absolute() {
            return None;
        }
        Some(self.plan_data_dir(&plan.id).join(rel_path))
    }

    pub fn attach_to_task(
        &self,
        plan_id: &str,
        task_id: &str,
        source: &Path,
        now_ms: u64,
    ) -> Result<AttachmentRef, PlanError> {
        let mut attached = None;
        let plan_root = self.plan_data_dir(plan_id);
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::Core,
            now_ms,
            |edit| {
                let task = &mut edit.task;
                if task.attachments.len() >= MAX_ATTACHMENTS_PER_RECORD {
                    return Err(PlanError::Io(format!(
                        "task attachment cap is {}",
                        MAX_ATTACHMENTS_PER_RECORD
                    )));
                }
                let canonical_task_id = task.id.clone();
                let attachment = copy_attachment(
                    &plan_root,
                    AttachmentTarget::Task,
                    &canonical_task_id,
                    source,
                    now_ms,
                )?;
                task.attachments.push(attachment.clone());
                task.updated_at = now_ms;
                attached = Some(attachment);
                Ok(())
            },
        )?;
        attached.ok_or_else(|| PlanError::Io("attachment was not recorded".into()))
    }

    pub fn attach_to_section(
        &self,
        plan_id: &str,
        section_id: &str,
        source: &Path,
        now_ms: u64,
    ) -> Result<AttachmentRef, PlanError> {
        let mut attached = None;
        let plan_root = self.plan_data_dir(plan_id);
        let turn = self.mutation_writer.enter();
        let result = self
            .database
            .edit_section_rows(plan_id, section_id, now_ms, |section| {
                if section.attachments.len() >= MAX_ATTACHMENTS_PER_RECORD {
                    return Err(PlanError::Io(format!(
                        "section attachment cap is {}",
                        MAX_ATTACHMENTS_PER_RECORD
                    )));
                }
                let attachment = copy_attachment(
                    &plan_root,
                    AttachmentTarget::Section,
                    &section.id,
                    source,
                    now_ms,
                )?;
                section.attachments.push(attachment.clone());
                attached = Some(attachment);
                Ok(())
            });
        match result {
            Ok(((), receipt)) => self.publish_hot_receipt(&turn, receipt),
            Err(error) => {
                if let Some(attachment) = attached {
                    let _ = std::fs::remove_file(plan_root.join(attachment.path));
                }
                return Err(error);
            }
        }
        attached.ok_or_else(|| PlanError::Io("attachment was not recorded".into()))
    }

    pub fn attach_to_milestone(
        &self,
        plan_id: &str,
        task_id: &str,
        milestone_id: &str,
        source: &Path,
        now_ms: u64,
    ) -> Result<AttachmentRef, PlanError> {
        let plan_root = self.plan_data_dir(plan_id);
        let turn = self.mutation_writer.enter();
        let (attachment, receipt) = self.database.with_milestone_rows(
            plan_id,
            task_id,
            milestone_id,
            now_ms,
            |milestone| {
                if milestone.attachments.len() >= MAX_ATTACHMENTS_PER_RECORD {
                    return Err(PlanError::Io(format!(
                        "milestone attachment cap is {}",
                        MAX_ATTACHMENTS_PER_RECORD
                    )));
                }
                let attachment = copy_attachment(
                    &plan_root,
                    AttachmentTarget::Milestone,
                    &milestone.id,
                    source,
                    now_ms,
                )?;
                milestone.attachments.push(attachment.clone());
                Ok(attachment)
            },
        )?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(attachment)
    }

    pub fn remove_task_attachment_dir(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<(), PlanError> {
        remove_dir_if_exists(attachment_dir(
            &self.plan_data_dir(plan_id),
            AttachmentTarget::Task,
            task_id,
        ))
    }

    pub fn remove_section_attachment_dir(
        &self,
        plan_id: &str,
        section_id: &str,
    ) -> Result<(), PlanError> {
        remove_dir_if_exists(attachment_dir(
            &self.plan_data_dir(plan_id),
            AttachmentTarget::Section,
            section_id,
        ))
    }

    pub fn orphan_attachment_dirs(
        &self,
        project_path: &Path,
    ) -> Result<Vec<OrphanAttachmentDir>, PlanError> {
        let project_root = paths::project_root(project_path);
        let read = self.database.attachment_reference_read()?;
        let mut out = Vec::new();
        for id in read.project_plan_ids(&project_root)? {
            let plan_root = self.plan_data_dir(&id);
            let mut directories = Vec::new();
            for target in [
                AttachmentTarget::Task,
                AttachmentTarget::Section,
                AttachmentTarget::Milestone,
            ] {
                collect_attachment_dirs(&plan_root, target, &mut directories);
            }
            let candidates = directories
                .iter()
                .map(|(target, id, _)| (target_dir(*target), id.as_str()))
                .collect::<Vec<_>>();
            let known = read.known_directories(&id, &candidates)?;
            for ((_, _, path), live) in directories.into_iter().zip(known) {
                if !live {
                    out.push(OrphanAttachmentDir {
                        plan_id: id.clone(),
                        path,
                    })
                }
            }
        }
        read.finish()?;
        Ok(out)
    }
}

fn collect_attachment_dirs(
    plan_root: &Path,
    target: AttachmentTarget,
    out: &mut Vec<(AttachmentTarget, String, PathBuf)>,
) {
    let root = attachment_dir(plan_root, target, "_")
        .parent()
        .map(Path::to_path_buf);
    let Some(root) = root else { return };
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            out.push((
                target,
                entry.file_name().to_string_lossy().to_string(),
                entry.path(),
            ))
        }
    }
}
