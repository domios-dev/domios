use crate::plan::{CanvasElement, Milestone, Plan, Section, Task};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
enum BodyKind {
    Section,
    Task,
    Handoff,
    Milestone,
    Element,
}

const MARKDOWN_SIDECAR_MIN_CHARS: usize = 240;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanFileEntry {
    pub kind: &'static str,
    pub title: String,
    pub path: String,
    pub exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_command: Option<String>,
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
            BodyKind::Handoff => "handoffs",
            BodyKind::Milestone => "milestones",
            BodyKind::Element => "elements",
        }
    }
}

pub fn plan_body_dir(plan: &Plan) -> std::io::Result<PathBuf> {
    super::paths::plan_state_dir(
        super::paths::plan_sidecar_root(plan),
        &plan.status,
        &plan.id,
    )
}

pub fn section_path(plan: &Plan, section_id: &str) -> std::io::Result<PathBuf> {
    let handle = flatten_sections(&plan.sections)
        .into_iter()
        .find(|section| section.id == section_id)
        .map(super::tag_model::section_handle)
        .unwrap_or(section_id);
    body_path(plan, BodyKind::Section, handle)
}

pub fn task_path(plan: &Plan, task_id: &str) -> std::io::Result<PathBuf> {
    let handle = plan
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .map(super::tag_model::task_handle)
        .unwrap_or(task_id);
    body_path(plan, BodyKind::Task, handle)
}

pub fn task_stride_log_path(plan: &Plan, task: &Task) -> std::io::Result<PathBuf> {
    Ok(plan_body_dir(plan)?
        .join("tasks")
        .join(super::tag_model::task_handle(task))
        .join("strides.jsonl"))
}

pub fn handoff_path(plan: &Plan, element_id: &str) -> std::io::Result<PathBuf> {
    body_path(plan, BodyKind::Handoff, element_id)
}

pub fn milestone_path(plan: &Plan, task: &Task, milestone: &Milestone) -> std::io::Result<PathBuf> {
    body_path(
        plan,
        BodyKind::Milestone,
        &format!("{}-{}", super::tag_model::task_handle(task), milestone.id),
    )
}

pub fn element_path(plan: &Plan, element: &CanvasElement) -> std::io::Result<PathBuf> {
    body_path(
        plan,
        BodyKind::Element,
        &format!("{}-{}", element.kind, element.id),
    )
}

pub fn index_path(plan: &Plan) -> std::io::Result<PathBuf> {
    Ok(plan_body_dir(plan)?.join("INDEX.md"))
}

pub fn goal_path(plan: &Plan) -> std::io::Result<PathBuf> {
    Ok(plan_body_dir(plan)?.join("goal.md"))
}

fn body_path(plan: &Plan, kind: BodyKind, id: &str) -> std::io::Result<PathBuf> {
    Ok(plan_body_dir(plan)?
        .join(kind.dir_name())
        .join(format!("{id}.md")))
}

pub fn strip_bodies(plan: &Plan) -> Plan {
    let mut stripped = plan.clone();
    if let Some(goal) = &mut stripped.goal {
        goal.vision.clear();
    }
    clear_section_bodies(&mut stripped.sections);
    for task in &mut stripped.tasks {
        task.body.clear();
        task.milestones.clear();
    }
    for element in &mut stripped.elements {
        if should_externalize_element(element) {
            clear_element_markdown(element);
        }
    }
    stripped
}

pub fn persist_bodies(plan: &Plan) -> std::io::Result<Vec<PathBuf>> {
    let paths = body_paths(plan)?;
    if let Some(goal) = &plan.goal {
        write_markdown_atomic(
            &goal_path(plan)?,
            &body_with_frontmatter(
                frontmatter(plan, "goal", &plan.id, "goal", Some("Goal"), None),
                &[],
                &goal.vision,
            ),
        )?;
    }
    for section in flatten_sections(&plan.sections) {
        let path = section_path(plan, &section.id)?;
        write_markdown_atomic(
            &path,
            &body_with_frontmatter(
                frontmatter(
                    plan,
                    "section",
                    &section.id,
                    super::tag_model::section_handle(section),
                    Some(&section.title),
                    None,
                ),
                &section.tags,
                &section.body,
            ),
        )?;
        remove_stale_id_path(plan, BodyKind::Section, &section.id, &path)?;
    }
    for task in &plan.tasks {
        let path = task_path(plan, &task.id)?;
        let mut metadata = frontmatter(
            plan,
            "task",
            &task.id,
            super::tag_model::task_handle(task),
            Some(&task.name),
            Some(task_status_str(&task.status)),
        );
        if let Some(role) = task
            .executor_role
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            metadata.insert(
                "executor_role".into(),
                serde_json::Value::String(role.to_string()),
            );
        }
        if let Some(name) = task
            .personality
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            metadata.insert(
                "personality".into(),
                serde_json::Value::String(name.to_string()),
            );
        }
        if let Some(provenance) = task.personality_provenance {
            metadata.insert(
                "personality_provenance".into(),
                serde_json::to_value(provenance).unwrap_or(serde_json::Value::Null),
            );
        }
        write_markdown_atomic(
            &path,
            &body_with_frontmatter(metadata, &task.tags, &task.body),
        )?;
        remove_stale_id_path(plan, BodyKind::Task, &task.id, &path)?;
        for milestone in &task.milestones {
            persist_optional_markdown(
                &milestone_path(plan, task, milestone)?,
                &body_with_frontmatter(
                    frontmatter(
                        plan,
                        "milestone",
                        &milestone.id,
                        &format!("{}-{}", super::tag_model::task_handle(task), milestone.id),
                        Some(&milestone.kind),
                        None,
                    ),
                    &[],
                    &milestone.text,
                ),
                should_externalize_markdown(&milestone.text),
            )?;
        }
        persist_stride_log(plan, task)?;
    }
    for element in &plan.elements {
        if element.kind == "handoff" {
            continue;
        }
        if let Some(body) = element_markdown(element) {
            persist_optional_markdown(
                &element_path(plan, element)?,
                &body_with_frontmatter(
                    frontmatter(
                        plan,
                        "element",
                        &element.id,
                        &format!("{}-{}", element.kind, element.id),
                        Some(&element.kind),
                        None,
                    ),
                    &[],
                    body,
                ),
                should_externalize_markdown(body),
            )?;
        }
    }
    write_index(plan)?;
    Ok(paths)
}

pub fn body_paths(plan: &Plan) -> std::io::Result<Vec<PathBuf>> {
    let mut paths = vec![];
    if plan.goal.is_some() {
        paths.push(goal_path(plan)?);
    }
    collect_section_paths(plan, &plan.sections, &mut paths)?;
    for task in &plan.tasks {
        paths.push(task_path(plan, &task.id)?);
        let stride_log_path = task_stride_log_path(plan, task)?;
        if !task.milestones.is_empty() || stride_log_path.exists() {
            paths.push(stride_log_path);
        }
        for milestone in &task.milestones {
            let path = milestone_path(plan, task, milestone)?;
            if should_externalize_markdown(&milestone.text) || path.exists() {
                paths.push(path);
            }
        }
    }
    for element in plan
        .elements
        .iter()
        .filter(|element| element.kind == "handoff")
    {
        paths.push(handoff_path(plan, &element.id)?);
    }
    for element in plan.elements.iter().filter(|element| {
        element.kind != "handoff"
            && (should_externalize_element(element)
                || element_path(plan, element).is_ok_and(|path| path.exists()))
    }) {
        paths.push(element_path(plan, element)?);
    }
    paths.push(index_path(plan)?);
    Ok(paths)
}

pub fn file_manifest(plan: &Plan) -> std::io::Result<PlanFileManifest> {
    let entries = index_entries(plan)?
        .into_iter()
        .map(|entry| PlanFileEntry {
            kind: entry.kind,
            title: entry.title,
            exists: entry.path.exists(),
            read_command: entry.read_command,
            path: display_path(&entry.path),
        })
        .collect();
    Ok(PlanFileManifest {
        index_path: display_path(&index_path(plan)?),
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
    super::tag_model::normalize_plan(plan);
    let base_dir = plan_body_dir(plan)?;
    if let Some(goal) = &mut plan.goal {
        let mut tags = vec![];
        migrated |= hydrate_body(&base_dir.join("goal.md"), None, &mut goal.vision, &mut tags)?;
    }
    hydrate_sections(&base_dir, &mut plan.sections, &mut migrated)?;
    for task in &mut plan.tasks {
        let path = base_dir
            .join("tasks")
            .join(format!("{}.md", super::tag_model::task_handle(task)));
        let fallback = base_dir.join("tasks").join(format!("{}.md", task.id));
        migrated |= hydrate_body(&path, Some(&fallback), &mut task.body, &mut task.tags)?;
        let task_handle = super::tag_model::task_handle(task).to_string();
        let stride_log_path = base_dir
            .join("tasks")
            .join(&task_handle)
            .join("strides.jsonl");
        if stride_log_path.exists() {
            let mut combined = task.milestones.clone();
            combined.extend(
                super::stride_log::read_milestones(&stride_log_path)
                    .map_err(stride_log_to_io_error)?,
            );
            let folded = super::stride_log::fold_milestones(combined);
            if folded != task.milestones {
                task.milestones = folded;
                migrated = true;
            }
        }
        for milestone in &mut task.milestones {
            let path = base_dir
                .join("milestones")
                .join(format!("{}-{}.md", task_handle, milestone.id));
            migrated |= hydrate_plain_markdown(&path, &mut milestone.text)?;
        }
    }
    for element in &mut plan.elements {
        if element.kind == "handoff" {
            continue;
        }
        let path = base_dir
            .join("elements")
            .join(format!("{}-{}.md", element.kind, element.id));
        migrated |= hydrate_element_markdown(&path, element)?;
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
    read_command: Option<String>,
}

fn index_entries(plan: &Plan) -> std::io::Result<Vec<IndexEntryRaw>> {
    let mut entries = vec![];
    if plan.goal.is_some() {
        entries.push(IndexEntryRaw {
            kind: "goal",
            title: "Goal".into(),
            path: goal_path(plan)?,
            index_link_path: "goal.md".into(),
            read_command: None,
        });
    }
    collect_section_index_entries(plan, &plan.sections, &mut entries)?;
    for task in &plan.tasks {
        entries.push(task_index_entry(plan, task)?);
        let stride_log_path = task_stride_log_path(plan, task)?;
        if !task.milestones.is_empty() || stride_log_path.exists() {
            entries.push(stride_log_index_entry(plan, task)?);
        }
        for milestone in &task.milestones {
            let path = milestone_path(plan, task, milestone)?;
            if should_externalize_markdown(&milestone.text) || path.exists() {
                entries.push(milestone_index_entry(plan, task, milestone)?);
            }
        }
    }
    for element in plan
        .elements
        .iter()
        .filter(|element| element.kind == "handoff")
    {
        entries.push(handoff_index_entry(plan, element)?);
    }
    for element in plan.elements.iter().filter(|element| {
        element.kind != "handoff"
            && (should_externalize_element(element)
                || element_path(plan, element).is_ok_and(|path| path.exists()))
    }) {
        entries.push(element_index_entry(plan, element)?);
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
            title: format!(
                "{} ({})",
                super::tag_model::section_handle(section),
                section.title
            ),
            path,
            index_link_path: format!("sections/{}.md", super::tag_model::section_handle(section)),
            read_command: Some(format!(
                "codeterm plan section show {} {}",
                plan.id, section.id
            )),
        });
        collect_section_index_entries(plan, &section.subsections, entries)?;
    }
    Ok(())
}

fn task_index_entry(plan: &Plan, task: &Task) -> std::io::Result<IndexEntryRaw> {
    Ok(IndexEntryRaw {
        kind: "task",
        title: format!("{} ({})", super::tag_model::task_handle(task), task.name),
        path: task_path(plan, &task.id)?,
        index_link_path: format!("tasks/{}.md", super::tag_model::task_handle(task)),
        read_command: Some(format!("codeterm plan task show {} {}", plan.id, task.id)),
    })
}

fn stride_log_index_entry(plan: &Plan, task: &Task) -> std::io::Result<IndexEntryRaw> {
    let task_handle = super::tag_model::task_handle(task);
    Ok(IndexEntryRaw {
        kind: "stride_log",
        title: format!("{} strides", task_handle),
        path: task_stride_log_path(plan, task)?,
        index_link_path: format!("tasks/{}/strides.jsonl", task_handle),
        read_command: None,
    })
}

fn handoff_index_entry(plan: &Plan, element: &CanvasElement) -> std::io::Result<IndexEntryRaw> {
    let seq = element
        .content
        .get("seq")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    Ok(IndexEntryRaw {
        kind: "handoff",
        title: format!("handoff #{} ({})", seq, element.id),
        path: handoff_path(plan, &element.id)?,
        index_link_path: format!("handoffs/{}.md", element.id),
        read_command: None,
    })
}

fn milestone_index_entry(
    plan: &Plan,
    task: &Task,
    milestone: &Milestone,
) -> std::io::Result<IndexEntryRaw> {
    let task_handle = super::tag_model::task_handle(task);
    Ok(IndexEntryRaw {
        kind: "milestone",
        title: format!("{} {} ({})", task_handle, milestone.kind, milestone.id),
        path: milestone_path(plan, task, milestone)?,
        index_link_path: format!("milestones/{}-{}.md", task_handle, milestone.id),
        read_command: None,
    })
}

fn element_index_entry(plan: &Plan, element: &CanvasElement) -> std::io::Result<IndexEntryRaw> {
    Ok(IndexEntryRaw {
        kind: "element",
        title: format!("{} ({})", element.kind, element.id),
        path: element_path(plan, element)?,
        index_link_path: format!("elements/{}-{}.md", element.kind, element.id),
        read_command: None,
    })
}

fn write_index(plan: &Plan) -> std::io::Result<()> {
    let entries = index_entries(plan)?;
    let mut out = body_with_frontmatter(
        frontmatter(plan, "index", &plan.id, "index", Some(&plan.title), None),
        &[],
        &format!("# {}\n", plan.title),
    );
    out.push('\n');
    write_index_group(&mut out, "Goal", &entries, "goal");
    write_index_group(&mut out, "Sections", &entries, "section");
    write_index_group(&mut out, "Tasks", &entries, "task");
    write_index_group(&mut out, "Stride Logs", &entries, "stride_log");
    write_index_group(&mut out, "Milestones", &entries, "milestone");
    write_index_group(&mut out, "Elements", &entries, "element");
    write_index_group(&mut out, "Handoffs", &entries, "handoff");
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

fn display_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn hydrate_sections(
    base_dir: &Path,
    sections: &mut [Section],
    migrated: &mut bool,
) -> std::io::Result<()> {
    for section in sections {
        let path = base_dir
            .join("sections")
            .join(format!("{}.md", super::tag_model::section_handle(section)));
        let fallback = base_dir.join("sections").join(format!("{}.md", section.id));
        *migrated |= hydrate_body(&path, Some(&fallback), &mut section.body, &mut section.tags)?;
        hydrate_sections(base_dir, &mut section.subsections, migrated)?;
    }
    Ok(())
}

fn hydrate_plain_markdown(path: &Path, body: &mut String) -> std::io::Result<bool> {
    let had_inline_body = !body.is_empty();
    if path.exists() {
        let Some(text) = read_markdown_sidecar(path) else {
            return Ok(false);
        };
        let (_, stripped) = parse_frontmatter_tags(&text);
        *body = stripped;
        return Ok(had_inline_body);
    }
    if should_externalize_markdown(body) {
        write_markdown_atomic(path, body)?;
        return Ok(true);
    }
    Ok(false)
}

fn hydrate_element_markdown(path: &Path, element: &mut CanvasElement) -> std::io::Result<bool> {
    let had_inline_body = should_externalize_element(element);
    if path.exists() {
        let Some(text) = read_markdown_sidecar(path) else {
            return Ok(false);
        };
        let (_, stripped) = parse_frontmatter_tags(&text);
        set_element_markdown(element, stripped);
        return Ok(had_inline_body);
    }
    if let Some(body) = element_markdown(element) {
        if should_externalize_markdown(body) {
            write_markdown_atomic(path, body)?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn hydrate_body(
    path: &Path,
    fallback: Option<&Path>,
    body: &mut String,
    tags: &mut Vec<String>,
) -> std::io::Result<bool> {
    let had_inline_body = !body.is_empty();
    let read_path = if path.exists() {
        Some(path)
    } else {
        fallback.filter(|fallback| fallback.exists())
    };
    if let Some(read_path) = read_path {
        let Some(text) = read_markdown_sidecar(read_path) else {
            return Ok(false);
        };
        let (frontmatter_tags, stripped) = parse_frontmatter_tags(&text);
        if !frontmatter_tags.is_empty() {
            *tags = frontmatter_tags;
        }
        *body = stripped;
        return Ok(had_inline_body);
    }
    if had_inline_body {
        write_markdown_atomic(
            path,
            &body_with_frontmatter(serde_json::Map::new(), tags, body),
        )?;
        return Ok(true);
    }
    Ok(false)
}

fn read_markdown_sidecar(path: &Path) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(err) => {
            log::warn!(
                "[plan] ignoring unreadable markdown sidecar {}: {}",
                path.display(),
                err
            );
            None
        }
    }
}

fn clear_section_bodies(sections: &mut [Section]) {
    for section in sections {
        section.body.clear();
        clear_section_bodies(&mut section.subsections);
    }
}

fn persist_optional_markdown(path: &Path, body: &str, should_persist: bool) -> std::io::Result<()> {
    if should_persist {
        write_markdown_atomic(path, body)
    } else {
        remove_file_if_exists(path)
    }
}

fn persist_stride_log(plan: &Plan, task: &Task) -> std::io::Result<()> {
    let path = task_stride_log_path(plan, task)?;
    let mut existing_ids = super::stride_log::read_milestones(&path)
        .map_err(stride_log_to_io_error)?
        .into_iter()
        .map(|milestone| milestone.id)
        .collect::<BTreeSet<_>>();
    for milestone in &task.milestones {
        if existing_ids.insert(milestone.id.clone()) {
            super::stride_log::append_milestone(&path, milestone)
                .map_err(stride_log_to_io_error)?;
        }
    }
    Ok(())
}

fn stride_log_to_io_error(error: super::stride_log::StrideLogError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
}

fn remove_file_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn write_markdown_atomic(path: &Path, body: &str) -> std::io::Result<()> {
    let normalized = normalize_markdown_eof(body);
    // Whole-plan saves fan out here for every entity; skipping identical
    // content is what keeps untouched task files out of git and off disk.
    if matches!(std::fs::read_to_string(path), Ok(existing) if existing == normalized) {
        return Ok(());
    }
    super::atomic_write::write_atomic(path, normalized)
}

fn remove_stale_id_path(
    plan: &Plan,
    kind: BodyKind,
    id: &str,
    current: &Path,
) -> std::io::Result<()> {
    let stale = body_path(plan, kind, id)?;
    if stale != current && stale.exists() {
        std::fs::remove_file(stale)?;
    }
    Ok(())
}

fn frontmatter(
    plan: &Plan,
    kind: &str,
    id: &str,
    handle: &str,
    title: Option<&str>,
    status: Option<&str>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut metadata = serde_json::Map::new();
    metadata.insert("plan_id".into(), serde_json::Value::String(plan.id.clone()));
    metadata.insert("kind".into(), serde_json::Value::String(kind.into()));
    metadata.insert("id".into(), serde_json::Value::String(id.into()));
    metadata.insert("handle".into(), serde_json::Value::String(handle.into()));
    if let Some(title) = title {
        metadata.insert("title".into(), serde_json::Value::String(title.into()));
    }
    if let Some(status) = status {
        metadata.insert("status".into(), serde_json::Value::String(status.into()));
    }
    metadata
}

fn task_status_str(status: &crate::plan::TaskStatus) -> &'static str {
    match status {
        crate::plan::TaskStatus::Pending => "pending",
        crate::plan::TaskStatus::InProgress => "in_progress",
        crate::plan::TaskStatus::InReview => "in_review",
        crate::plan::TaskStatus::Blocked => "blocked",
        crate::plan::TaskStatus::Paused => "paused",
        crate::plan::TaskStatus::Done => "done",
        crate::plan::TaskStatus::Cancelled => "cancelled",
    }
}

fn body_with_frontmatter(
    mut metadata: serde_json::Map<String, serde_json::Value>,
    tags: &[String],
    body: &str,
) -> String {
    if !tags.is_empty() {
        metadata.insert("tags".into(), serde_json::json!(tags));
    }
    if metadata.is_empty() {
        return body.to_string();
    }
    let yaml = serde_yaml::to_string(&serde_json::Value::Object(metadata)).unwrap_or_default();
    format!("---\n{}---\n\n{}", yaml, body.trim_start())
}

fn normalize_markdown_eof(body: &str) -> String {
    format!("{}\n", body.trim_end_matches([' ', '\t', '\r', '\n']))
}

fn should_externalize_markdown(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.len() >= MARKDOWN_SIDECAR_MIN_CHARS || trimmed.contains('\n')
}

fn should_externalize_element(element: &CanvasElement) -> bool {
    element_markdown(element).is_some_and(should_externalize_markdown)
}

fn element_markdown(element: &CanvasElement) -> Option<&str> {
    if matches!(element.kind.as_str(), "note" | "decision") {
        if let Some(text) = element.content.as_str() {
            return Some(text);
        }
    }
    if !matches!(element.kind.as_str(), "note" | "decision" | "discussion") {
        return None;
    }
    element
        .content
        .as_object()
        .and_then(|obj| markdown_field_key(obj).and_then(|key| obj.get(key)))
        .and_then(|value| value.as_str())
}

fn clear_element_markdown(element: &mut CanvasElement) {
    set_element_markdown(element, String::new());
}

fn set_element_markdown(element: &mut CanvasElement, body: String) {
    if matches!(element.kind.as_str(), "note" | "decision") && element.content.is_string() {
        element.content = serde_json::Value::String(body);
        return;
    }
    let Some(obj) = element.content.as_object_mut() else {
        return;
    };
    let key = markdown_field_key(obj).unwrap_or("text");
    obj.insert(key.into(), serde_json::Value::String(body));
}

fn markdown_field_key(obj: &serde_json::Map<String, serde_json::Value>) -> Option<&'static str> {
    ["text", "body", "markdown"]
        .into_iter()
        .find(|key| obj.get(*key).is_some_and(|value| value.is_string()))
}

fn parse_frontmatter_tags(text: &str) -> (Vec<String>, String) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return (vec![], text.to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return (vec![], text.to_string());
    };
    let yaml = &rest[..end];
    let body = rest[end + "\n---".len()..].trim_start_matches('\n');
    let tags = serde_yaml::from_str::<serde_yaml::Value>(yaml)
        .ok()
        .and_then(|value| value.get("tags").cloned())
        .and_then(|value| serde_yaml::from_value::<Vec<String>>(value).ok())
        .unwrap_or_default();
    (tags, body.to_string())
}

#[cfg(test)]
mod write_if_changed_tests {
    use super::write_markdown_atomic;

    // Unix-only: the read-only-parent oracle needs mode bits; on Windows a
    // read-only directory does not block file creation at all.
    #[cfg(unix)]
    #[test]
    fn identical_content_skips_the_write_entirely() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("task.md");
        write_markdown_atomic(&path, "body text").unwrap();
        // A read-only parent makes any attempted write fail loudly, so Ok()
        // proves the identical-content branch never touched the filesystem.
        let mut perms = std::fs::metadata(dir.path()).unwrap().permissions();
        std::fs::set_permissions(dir.path(), {
            use std::os::unix::fs::PermissionsExt;
            let mut p = perms.clone();
            p.set_mode(0o555);
            p
        })
        .unwrap();
        let result = write_markdown_atomic(&path, "body text");
        // restore before asserting so the tempdir can clean up on failure too
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
        }
        std::fs::set_permissions(dir.path(), perms).unwrap();
        result.unwrap();
    }

    #[test]
    fn changed_content_still_writes_and_normalizes_eof() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("task.md");
        write_markdown_atomic(&path, "v1").unwrap();
        write_markdown_atomic(&path, "v2  \n\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "v2\n");
    }

    #[test]
    fn eof_normalized_equivalence_counts_as_identical() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("task.md");
        write_markdown_atomic(&path, "stable body").unwrap();
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        write_markdown_atomic(&path, "stable body\n\n  ").unwrap();
        let after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(before, after, "normalized-equal content must not rewrite");
    }
}
