use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::{paths, PlanError, PlanStore};
use crate::plan::{AttachmentRef, Plan, Section};

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
    std::fs::copy(source, &dest).map_err(|e| PlanError::Io(e.to_string()))?;
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
        size_bytes: meta.len(),
        created_at: now_ms,
    })
}

fn section_mut<'a>(sections: &'a mut [Section], id: &str) -> Option<&'a mut Section> {
    for section in sections {
        if section.id == id || section.handle.as_deref() == Some(id) {
            return Some(section);
        }
        if let Some(found) = section_mut(&mut section.subsections, id) {
            return Some(found);
        }
    }
    None
}

fn section_ref<'a>(sections: &'a [Section], id: &str) -> Option<&'a Section> {
    for section in sections {
        if section.id == id || section.handle.as_deref() == Some(id) {
            return Some(section);
        }
        if let Some(found) = section_ref(&section.subsections, id) {
            return Some(found);
        }
    }
    None
}

fn collect_section_ids(sections: &[Section], out: &mut HashSet<String>) {
    for section in sections {
        out.insert(section.id.clone());
        collect_section_ids(&section.subsections, out);
    }
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
        self.with_plan(plan_id, now_ms, |plan| {
            let pos = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            if plan.tasks[pos].attachments.len() >= MAX_ATTACHMENTS_PER_RECORD {
                return Err(PlanError::Io(format!(
                    "task attachment cap is {}",
                    MAX_ATTACHMENTS_PER_RECORD
                )));
            }
            let canonical_task_id = plan.tasks[pos].id.clone();
            let attachment = copy_attachment(
                &plan_root,
                AttachmentTarget::Task,
                &canonical_task_id,
                source,
                now_ms,
            )?;
            let task = plan.tasks.get_mut(pos).ok_or(PlanError::TaskNotFound)?;
            task.attachments.push(attachment.clone());
            task.updated_at = now_ms;
            attached = Some(attachment);
            Ok(())
        })?;
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
        self.with_plan(plan_id, now_ms, |plan| {
            let section =
                section_ref(&plan.sections, section_id).ok_or(PlanError::ElementNotFound)?;
            if section.attachments.len() >= MAX_ATTACHMENTS_PER_RECORD {
                return Err(PlanError::Io(format!(
                    "section attachment cap is {}",
                    MAX_ATTACHMENTS_PER_RECORD
                )));
            }
            let sid = section.id.clone();
            let attachment =
                copy_attachment(&plan_root, AttachmentTarget::Section, &sid, source, now_ms)?;
            let section =
                section_mut(&mut plan.sections, &sid).ok_or(PlanError::ElementNotFound)?;
            section.attachments.push(attachment.clone());
            attached = Some(attachment);
            Ok(())
        })?;
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
        let mut attached = None;
        let plan_root = self.plan_data_dir(plan_id);
        self.with_plan(plan_id, now_ms, |plan| {
            let task_pos = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let milestone_pos = plan.tasks[task_pos]
                .milestones
                .iter()
                .position(|milestone| milestone.id == milestone_id)
                .ok_or(PlanError::ElementNotFound)?;
            if plan.tasks[task_pos].milestones[milestone_pos]
                .attachments
                .len()
                >= MAX_ATTACHMENTS_PER_RECORD
            {
                return Err(PlanError::Io(format!(
                    "milestone attachment cap is {}",
                    MAX_ATTACHMENTS_PER_RECORD
                )));
            }
            let attachment = copy_attachment(
                &plan_root,
                AttachmentTarget::Milestone,
                milestone_id,
                source,
                now_ms,
            )?;
            let task = plan
                .tasks
                .get_mut(task_pos)
                .ok_or(PlanError::TaskNotFound)?;
            let milestone = task
                .milestones
                .get_mut(milestone_pos)
                .ok_or(PlanError::ElementNotFound)?;
            milestone.attachments.push(attachment.clone());
            task.updated_at = now_ms;
            attached = Some(attachment);
            Ok(())
        })?;
        attached.ok_or_else(|| PlanError::Io("attachment was not recorded".into()))
    }

    pub fn remove_task_attachment_dir(&self, plan: &Plan, task_id: &str) -> Result<(), PlanError> {
        remove_dir_if_exists(attachment_dir(
            &self.plan_data_dir(&plan.id),
            AttachmentTarget::Task,
            task_id,
        ))
    }

    pub fn remove_section_attachment_dir(
        &self,
        plan: &Plan,
        section_id: &str,
    ) -> Result<(), PlanError> {
        remove_dir_if_exists(attachment_dir(
            &self.plan_data_dir(&plan.id),
            AttachmentTarget::Section,
            section_id,
        ))
    }

    pub fn orphan_attachment_dirs(&self, project_path: &Path) -> Vec<OrphanAttachmentDir> {
        let mut out = vec![];
        let project_root = paths::project_root(project_path);
        for plan in self.list_full() {
            if paths::plan_sidecar_root(&plan) != project_root.as_path() {
                continue;
            }
            let mut task_ids = HashSet::new();
            let mut milestone_ids = HashSet::new();
            for task in &plan.tasks {
                task_ids.insert(task.id.clone());
                for milestone in &task.milestones {
                    milestone_ids.insert(milestone.id.clone());
                }
            }
            let mut section_ids = HashSet::new();
            collect_section_ids(&plan.sections, &mut section_ids);
            let plan_root = self.plan_data_dir(&plan.id);
            collect_orphans(
                &plan_root,
                &plan.id,
                AttachmentTarget::Task,
                &task_ids,
                &mut out,
            );
            collect_orphans(
                &plan_root,
                &plan.id,
                AttachmentTarget::Section,
                &section_ids,
                &mut out,
            );
            collect_orphans(
                &plan_root,
                &plan.id,
                AttachmentTarget::Milestone,
                &milestone_ids,
                &mut out,
            );
        }
        out
    }
}

fn collect_orphans(
    plan_root: &Path,
    plan_id: &str,
    target: AttachmentTarget,
    live_ids: &HashSet<String>,
    out: &mut Vec<OrphanAttachmentDir>,
) {
    let root = attachment_dir(plan_root, target, "_")
        .parent()
        .map(Path::to_path_buf);
    let Some(root) = root else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        if !live_ids.contains(&id) {
            out.push(OrphanAttachmentDir {
                plan_id: plan_id.to_string(),
                path: entry.path(),
            });
        }
    }
}
