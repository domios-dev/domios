use super::*;

#[derive(Debug, serde::Deserialize)]
pub struct FileSectionInput {
    pub id: String,
    pub handle: Option<String>,
    pub title: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct FileMilestoneInput {
    pub id: String,
    pub kind: String,
    pub externalize: bool,
}

#[derive(Debug, serde::Deserialize)]
pub struct FileTaskInput {
    pub id: String,
    pub handle: Option<String>,
    pub name: String,
    pub has_milestones: bool,
    #[serde(default)]
    pub milestones: Vec<FileMilestoneInput>,
}

#[derive(Debug)]
pub struct FileElementInput {
    pub id: String,
    pub kind: String,
    pub sequence: u64,
    pub externalize: bool,
}

#[derive(Debug)]
pub struct FileManifestInput {
    pub id: String,
    pub directory: PathBuf,
    pub has_goal: bool,
    pub sections: Vec<FileSectionInput>,
    pub tasks: Vec<FileTaskInput>,
    pub elements: Vec<FileElementInput>,
}

impl FileManifestInput {
    pub(in crate::service_plan) fn from_plan(plan: &Plan) -> std::io::Result<Self> {
        Ok(Self {
            id: plan.id.clone(),
            directory: plan_body_dir(plan)?,
            has_goal: plan.goal.is_some(),
            sections: flatten_sections(&plan.sections)
                .into_iter()
                .map(|section| FileSectionInput {
                    id: section.id.clone(),
                    handle: section.handle.clone(),
                    title: section.title.clone(),
                })
                .collect(),
            tasks: plan
                .tasks
                .iter()
                .map(|task| FileTaskInput {
                    id: task.id.clone(),
                    handle: task.handle.clone(),
                    name: task.name.clone(),
                    has_milestones: !task.milestones.is_empty(),
                    milestones: task
                        .milestones
                        .iter()
                        .map(|milestone| FileMilestoneInput {
                            id: milestone.id.clone(),
                            kind: milestone.kind.clone(),
                            externalize: should_externalize_markdown(&milestone.text),
                        })
                        .collect(),
                })
                .collect(),
            elements: plan
                .elements
                .iter()
                .map(|element| FileElementInput {
                    id: element.id.clone(),
                    kind: element.kind.clone(),
                    sequence: element
                        .content
                        .get("seq")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                    externalize: should_externalize_element(element),
                })
                .collect(),
        })
    }
}

pub(super) struct IndexEntryRaw {
    pub(super) kind: &'static str,
    pub(super) title: String,
    pub(super) path: PathBuf,
    pub(super) index_link_path: String,
    pub(super) read_command: Option<String>,
}

fn entry(
    input: &FileManifestInput,
    kind: &'static str,
    title: String,
    relative: String,
    command: Option<String>,
) -> IndexEntryRaw {
    IndexEntryRaw {
        kind,
        title,
        path: input.directory.join(&relative),
        index_link_path: relative,
        read_command: command,
    }
}

pub(super) fn index_entries(input: &FileManifestInput) -> Vec<IndexEntryRaw> {
    let mut entries = Vec::new();
    let mut section_paths = std::collections::HashMap::new();
    let mut task_paths = std::collections::HashMap::new();
    if input.has_goal {
        entries.push(entry(input, "goal", "Goal".into(), "goal.md".into(), None));
    }
    for section in &input.sections {
        let handle = section.handle.as_deref().unwrap_or(&section.id);
        let path_handle = section_paths.entry(section.id.as_str()).or_insert(handle);
        let mut section_entry = entry(
            input,
            "section",
            format!("{handle} ({})", section.title),
            format!("sections/{handle}.md"),
            Some(format!(
                "domios plan section show {} {}",
                input.id, section.id
            )),
        );
        section_entry.path = input.directory.join(format!("sections/{path_handle}.md"));
        entries.push(section_entry);
    }
    for task in &input.tasks {
        let handle = task.handle.as_deref().unwrap_or(&task.id);
        let path_handle = task_paths.entry(task.id.as_str()).or_insert(handle);
        let mut task_entry = entry(
            input,
            "task",
            format!("{handle} ({})", task.name),
            format!("tasks/{handle}.md"),
            Some(format!("domios plan task show {} {}", input.id, task.id)),
        );
        task_entry.path = input.directory.join(format!("tasks/{path_handle}.md"));
        entries.push(task_entry);
        let relative = format!("tasks/{handle}/strides.jsonl");
        if task.has_milestones || input.directory.join(&relative).exists() {
            entries.push(entry(
                input,
                "stride_log",
                format!("{handle} strides"),
                relative,
                None,
            ));
        }
        for milestone in &task.milestones {
            let relative = format!("milestones/{handle}-{}.md", milestone.id);
            if milestone.externalize || input.directory.join(&relative).exists() {
                entries.push(entry(
                    input,
                    "milestone",
                    format!("{handle} {} ({})", milestone.kind, milestone.id),
                    relative,
                    None,
                ));
            }
        }
    }
    for element in input
        .elements
        .iter()
        .filter(|element| element.kind == "handoff")
    {
        entries.push(entry(
            input,
            "handoff",
            format!("handoff #{} ({})", element.sequence, element.id),
            format!("handoffs/{}.md", element.id),
            None,
        ));
    }
    for element in input
        .elements
        .iter()
        .filter(|element| element.kind != "handoff")
    {
        let relative = format!("elements/{}-{}.md", element.kind, element.id);
        if element.externalize || input.directory.join(&relative).exists() {
            entries.push(entry(
                input,
                "element",
                format!("{} ({})", element.kind, element.id),
                relative,
                None,
            ));
        }
    }
    entries
}
