use crate::plan::{Plan, Section, Task};
use serde::Serialize;
use std::path::{Path, PathBuf};

enum BodyKind {
    Section,
    Task,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanFileEntry {
    pub kind: &'static str,
    pub title: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanFileManifest {
    pub index_path: String,
    pub entries: Vec<PlanFileEntry>,
}

impl BodyKind {
    fn dir_name(&self) -> &'static str {
        match self {
            BodyKind::Section => "sections",
            BodyKind::Task => "tasks",
        }
    }
}

pub fn plan_body_dir(plan: &Plan) -> std::io::Result<PathBuf> {
    super::paths::plans_dir(Path::new(&plan.project_path)).map(|dir| dir.join(&plan.id))
}

pub fn section_path(plan: &Plan, section_id: &str) -> std::io::Result<PathBuf> {
    body_path(plan, BodyKind::Section, section_id)
}

pub fn task_path(plan: &Plan, task_id: &str) -> std::io::Result<PathBuf> {
    body_path(plan, BodyKind::Task, task_id)
}

pub fn index_path(plan: &Plan) -> std::io::Result<PathBuf> {
    Ok(plan_body_dir(plan)?.join("INDEX.md"))
}

fn body_path(plan: &Plan, kind: BodyKind, id: &str) -> std::io::Result<PathBuf> {
    Ok(plan_body_dir(plan)?
        .join(kind.dir_name())
        .join(format!("{id}.md")))
}

pub fn strip_bodies(plan: &Plan) -> Plan {
    let mut stripped = plan.clone();
    clear_section_bodies(&mut stripped.sections);
    for task in &mut stripped.tasks {
        task.body.clear();
    }
    stripped
}

pub fn persist_bodies(plan: &Plan) -> std::io::Result<Vec<PathBuf>> {
    let paths = body_paths(plan)?;
    for section in flatten_sections(&plan.sections) {
        let path = section_path(plan, &section.id)?;
        write_markdown_atomic(&path, &section.body)?;
    }
    for task in &plan.tasks {
        let path = task_path(plan, &task.id)?;
        write_markdown_atomic(&path, &task.body)?;
    }
    write_index(plan)?;
    Ok(paths)
}

pub fn body_paths(plan: &Plan) -> std::io::Result<Vec<PathBuf>> {
    let mut paths = vec![];
    collect_section_paths(plan, &plan.sections, &mut paths)?;
    for task in &plan.tasks {
        paths.push(task_path(plan, &task.id)?);
    }
    paths.push(index_path(plan)?);
    Ok(paths)
}

pub fn file_manifest(plan: &Plan) -> std::io::Result<PlanFileManifest> {
    let project = Path::new(&plan.project_path);
    let entries = index_entries(plan)?
        .into_iter()
        .map(|entry| PlanFileEntry {
            kind: entry.kind,
            title: entry.title,
            path: project_relative(project, &entry.path),
        })
        .collect();
    Ok(PlanFileManifest {
        index_path: project_relative(project, &index_path(plan)?),
        entries,
    })
}

pub fn spec_markdown(plan: &Plan) -> String {
    let mut out = String::new();
    render_sections(&plan.sections, 1, &mut out);
    out
}

pub fn hydrate_and_migrate(plan: &mut Plan) -> std::io::Result<bool> {
    let mut migrated = false;
    let base_dir = plan_body_dir(plan)?;
    hydrate_sections(&base_dir, &mut plan.sections, &mut migrated)?;
    for task in &mut plan.tasks {
        let path = base_dir.join("tasks").join(format!("{}.md", task.id));
        migrated |= hydrate_body(&path, &mut task.body)?;
    }
    Ok(migrated)
}

pub fn remove_body_dir(plan: &Plan) -> std::io::Result<()> {
    let dir = plan_body_dir(plan)?;
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn collect_section_paths(
    plan: &Plan,
    sections: &[Section],
    paths: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for section in sections {
        paths.push(section_path(plan, &section.id)?);
        collect_section_paths(plan, &section.subsections, paths)?;
    }
    Ok(())
}

fn flatten_sections(sections: &[Section]) -> Vec<&Section> {
    let mut out = vec![];
    for section in sections {
        out.push(section);
        out.extend(flatten_sections(&section.subsections));
    }
    out
}

struct IndexEntryRaw {
    kind: &'static str,
    title: String,
    path: PathBuf,
    index_link_path: String,
}

fn index_entries(plan: &Plan) -> std::io::Result<Vec<IndexEntryRaw>> {
    let mut entries = vec![];
    collect_section_index_entries(plan, &plan.sections, &mut entries)?;
    for task in &plan.tasks {
        entries.push(task_index_entry(plan, task)?);
    }
    Ok(entries)
}

fn collect_section_index_entries(
    plan: &Plan,
    sections: &[Section],
    entries: &mut Vec<IndexEntryRaw>,
) -> std::io::Result<()> {
    for section in sections {
        let path = section_path(plan, &section.id)?;
        entries.push(IndexEntryRaw {
            kind: "section",
            title: section.title.clone(),
            path,
            index_link_path: format!("sections/{}.md", section.id),
        });
        collect_section_index_entries(plan, &section.subsections, entries)?;
    }
    Ok(())
}

fn task_index_entry(plan: &Plan, task: &Task) -> std::io::Result<IndexEntryRaw> {
    Ok(IndexEntryRaw {
        kind: "task",
        title: task.name.clone(),
        path: task_path(plan, &task.id)?,
        index_link_path: format!("tasks/{}.md", task.id),
    })
}

fn write_index(plan: &Plan) -> std::io::Result<()> {
    let entries = index_entries(plan)?;
    let mut out = format!("# {}\n\n", plan.title);
    write_index_group(&mut out, "Sections", &entries, "section");
    write_index_group(&mut out, "Tasks", &entries, "task");
    write_markdown_atomic(&index_path(plan)?, &out)
}

fn write_index_group(out: &mut String, heading: &str, entries: &[IndexEntryRaw], kind: &str) {
    out.push_str(&format!("## {heading}\n\n"));
    let mut any = false;
    for entry in entries.iter().filter(|entry| entry.kind == kind) {
        any = true;
        out.push_str(&format!(
            "- [{}]({})\n",
            escape_markdown_link_text(&entry.title),
            entry.index_link_path
        ));
    }
    if !any {
        out.push_str("- (none)\n");
    }
    out.push('\n');
}

fn render_sections(sections: &[Section], depth: usize, out: &mut String) {
    for section in sections {
        let level = "#".repeat(depth.min(6));
        out.push_str(&format!("{level} {}\n\n", section.title));
        if !section.body.is_empty() {
            out.push_str(&section.body);
            out.push_str("\n\n");
        }
        render_sections(&section.subsections, depth + 1, out);
    }
}

fn escape_markdown_link_text(text: &str) -> String {
    text.replace('\\', "\\\\").replace(']', "\\]")
}

fn project_relative(project: &Path, path: &Path) -> String {
    path.strip_prefix(project)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn hydrate_sections(
    base_dir: &Path,
    sections: &mut [Section],
    migrated: &mut bool,
) -> std::io::Result<()> {
    for section in sections {
        let path = base_dir.join("sections").join(format!("{}.md", section.id));
        *migrated |= hydrate_body(&path, &mut section.body)?;
        hydrate_sections(base_dir, &mut section.subsections, migrated)?;
    }
    Ok(())
}

fn hydrate_body(path: &Path, body: &mut String) -> std::io::Result<bool> {
    let had_inline_body = !body.is_empty();
    if path.exists() {
        *body = std::fs::read_to_string(path)?;
        return Ok(had_inline_body);
    }
    if had_inline_body {
        write_markdown_atomic(path, body)?;
        return Ok(true);
    }
    Ok(false)
}

fn clear_section_bodies(sections: &mut [Section]) {
    for section in sections {
        section.body.clear();
        clear_section_bodies(&mut section.subsections);
    }
}

pub fn write_markdown_atomic(path: &Path, body: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
