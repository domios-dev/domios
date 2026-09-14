use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{NewTask, PlanError, PlanStore};
use crate::plan::{
    AgentRef, CanvasElement, ElementCanvas, Goal, IdentityProvenance, Milestone, MilestoneKind,
    Plan, PlanStatus, Section, Spec, Task, TaskPriority, TaskStatus, TransferRecord,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BatchOp {
    UpsertTask {
        #[serde(default)]
        client_id: Option<String>,
        handle: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(rename = "type", default)]
        kind: Option<String>,
        #[serde(default)]
        sections: Vec<String>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        why: Option<String>,
        #[serde(default)]
        priority: Option<String>,
        #[serde(default)]
        group: Option<String>,
        #[serde(default)]
        personality: Option<String>,
        #[serde(default)]
        personality_provenance: Option<IdentityProvenance>,
    },
    SetTaskBody {
        task: String,
        #[serde(alias = "body_md")]
        body: String,
    },
    IngestSections {
        #[serde(alias = "markdown")]
        body: String,
    },
    SetTaskSections {
        #[serde(default)]
        client_id: Option<String>,
        task: String,
        #[serde(default)]
        sections: Vec<String>,
    },
    SetTaskStatus {
        #[serde(default)]
        client_id: Option<String>,
        task: String,
        status: String,
    },
    SetGoal {
        #[serde(default)]
        client_id: Option<String>,
        vector: String,
        #[serde(default)]
        vision: String,
        #[serde(default)]
        constraints: Vec<String>,
        #[serde(default)]
        postulates: Vec<String>,
    },
    AddMilestone {
        #[serde(default)]
        client_id: Option<String>,
        task: String,
        kind: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        from: Option<AgentRef>,
        #[serde(default)]
        key: Option<String>,
        #[serde(default)]
        links: Vec<String>,
    },
    AddElement {
        #[serde(default)]
        client_id: Option<String>,
        kind: String,
        #[serde(default)]
        content: Value,
        #[serde(default)]
        link: Option<String>,
        #[serde(default)]
        links: Vec<String>,
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
    },
    AddHandoff {
        #[serde(default)]
        client_id: Option<String>,
        #[serde(default)]
        mode: Option<String>,
        #[serde(default)]
        content: Value,
        #[serde(alias = "body_md", default)]
        body: String,
        #[serde(default)]
        task: Option<String>,
        #[serde(default)]
        from: Option<AgentRef>,
        #[serde(default)]
        to: Option<AgentRef>,
    },
    Link {
        to: String,
    },
    UpsertSpec {
        #[serde(default)]
        client_id: Option<String>,
        #[serde(default)]
        spec_id: Option<String>,
        title: String,
    },
    UpsertSpecSection {
        #[serde(default)]
        client_id: Option<String>,
        spec: String,
        #[serde(default)]
        section_id: Option<String>,
        title: String,
        #[serde(default)]
        body: Option<String>,
    },
    SetSpecSectionBody {
        spec: String,
        section: String,
        #[serde(alias = "body_md")]
        body: String,
    },
    LinkSpecSection {
        spec: String,
        spec_section: String,
        plan_section: String,
    },
    UnlinkSpecSection {
        spec: String,
        #[serde(default)]
        spec_section: Option<String>,
        #[serde(default)]
        plan_section: Option<String>,
    },
    CreatePlanFromSpec {
        #[serde(default)]
        client_id: Option<String>,
        spec: String,
        title: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatchRequest {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub ops: Vec<BatchOp>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BatchResult {
    pub ok: bool,
    pub ids: HashMap<String, String>,
    pub task_ids: Vec<String>,
    pub milestone_ids: Vec<String>,
    pub element_ids: Vec<String>,
    pub spec_ids: Vec<String>,
    pub spec_section_ids: Vec<String>,
    #[serde(rename = "_hints", skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
}

impl Default for BatchResult {
    fn default() -> Self {
        Self {
            ok: true,
            ids: HashMap::new(),
            task_ids: Vec::new(),
            milestone_ids: Vec::new(),
            element_ids: Vec::new(),
            spec_ids: Vec::new(),
            spec_section_ids: Vec::new(),
            hints: Vec::new(),
        }
    }
}

fn gen_id(prefix: &str) -> String {
    crate::plan::ids::short_id(prefix)
}

fn parse_status(raw: &str) -> Result<TaskStatus, PlanError> {
    serde_json::from_value(Value::String(raw.into())).map_err(|_| PlanError::BadStatus)
}

fn parse_priority(raw: &str) -> Result<TaskPriority, PlanError> {
    serde_json::from_value(Value::String(raw.into())).map_err(|_| PlanError::BadPriority)
}

fn resolve_ref(value: &str, ids: &HashMap<String, String>) -> String {
    value
        .strip_prefix('$')
        .and_then(|key| ids.get(key))
        .cloned()
        .unwrap_or_else(|| value.to_string())
}

fn find_task_mut<'a>(tasks: &'a mut [Task], selector: &str) -> Option<&'a mut Task> {
    if selector == "@latest" {
        return tasks
            .iter_mut()
            .max_by_key(|task| (task.updated_at, task.created_at));
    }
    crate::plan::task_key::find_task_mut(tasks, selector)
}

fn find_task_index(tasks: &[Task], selector: &str) -> Option<usize> {
    if selector == "@latest" {
        return tasks
            .iter()
            .enumerate()
            .max_by_key(|(_, task)| (task.updated_at, task.created_at))
            .map(|(index, _)| index);
    }
    crate::plan::task_key::find_task_index(tasks, selector)
}

/// Upserts that will create a task; keys are issued for them before the plan lock.
fn created_task_count(plan: Option<&Plan>, request: &BatchRequest) -> usize {
    let existing = plan.map(|plan| plan.tasks.as_slice()).unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    request
        .ops
        .iter()
        .filter_map(|op| match op {
            BatchOp::UpsertTask { handle, .. } => Some(handle.as_str()),
            _ => None,
        })
        .filter(|handle| find_task_index(existing, handle).is_none() && seen.insert(*handle))
        .count()
}

fn ensure_kind(plan_kinds: &mut Vec<MilestoneKind>, kind: &str) {
    if !plan_kinds.iter().any(|item| item.key == kind) {
        plan_kinds.push(MilestoneKind {
            key: kind.into(),
            label: kind.into(),
            color: None,
        });
    }
}

fn milestone_key(op_key: Option<&str>, request_key: Option<&str>) -> Option<String> {
    op_key.or(request_key).map(|key| format!("key:{key}"))
}

fn next_handoff_seq(plan: &crate::plan::Plan) -> u64 {
    plan.elements
        .iter()
        .filter(|element| element.kind == "handoff")
        .filter_map(|element| element.content.get("seq").and_then(|v| v.as_u64()))
        .max()
        .unwrap_or(0)
        + 1
}

fn staged_spec<'a>(
    staged_specs: &'a mut HashMap<String, Spec>,
    store: &PlanStore,
    spec_id: &str,
) -> Result<&'a mut Spec, PlanError> {
    if !staged_specs.contains_key(spec_id) {
        let spec = store.get_spec(spec_id).ok_or(PlanError::NotFound)?;
        staged_specs.insert(spec_id.to_string(), spec);
    }
    staged_specs.get_mut(spec_id).ok_or(PlanError::NotFound)
}

fn find_section_mut<'a>(sections: &'a mut [Section], id: &str) -> Option<&'a mut Section> {
    for section in sections {
        if section.id == id {
            return Some(section);
        }
        if let Some(found) = find_section_mut(&mut section.subsections, id) {
            return Some(found);
        }
    }
    None
}

impl PlanStore {
    /// Applies ops as given: it does not check who is asking. Every entrance goes
    /// through `service::plan_batch_authority` first, and a new one must too.
    pub fn apply_batch(
        &self,
        plan_id: &str,
        request: BatchRequest,
        now_ms: u64,
    ) -> Result<BatchResult, PlanError> {
        let created_count = created_task_count(self.get(plan_id).as_ref(), &request);
        let mut issued_keys = self.issue_task_keys(plan_id, created_count).into_iter();
        let (result, pending_markdown_writes, staged_specs, staged_plans) =
            self.with_plan(plan_id, now_ms, |plan| {
                let mut working = plan.clone();
                let mut result = BatchResult::default();
                let mut pending_markdown_writes: Vec<(PathBuf, String)> = Vec::new();
                let mut staged_specs: HashMap<String, Spec> = HashMap::new();
                let mut staged_plans: Vec<Plan> = Vec::new();
                for op in request.ops {
                    match op {
                        BatchOp::UpsertTask {
                            client_id,
                            handle,
                            name,
                            kind,
                            sections,
                            status,
                            body,
                            model,
                            why,
                            priority,
                            group,
                            personality,
                            personality_provenance: _,
                        } => {
                            let created =
                                crate::plan::task_key::find_task_index(&working.tasks, &handle)
                                    .is_none();
                            let missing_model = model.as_deref().is_none_or(str::is_empty);
                            let missing_why = why.as_deref().is_none_or(str::is_empty);
                            let missing_sections = sections.is_empty();
                            let task_id = if let Some(task_index) =
                                find_task_index(&working.tasks, &handle)
                            {
                                let parsed_status =
                                    status.as_deref().map(parse_status).transpose()?;
                                if let Some(next) = &parsed_status {
                                    let task_id = working.tasks[task_index].id.clone();
                                    super::mutations::validate_task_status_transition(
                                        &working,
                                        &task_id,
                                        &working.tasks[task_index].status,
                                        next,
                                        None,
                                    )?;
                                }
                                let task = &mut working.tasks[task_index];
                                if let Some(name) = name {
                                    task.name = name;
                                }
                                if let Some(kind) = kind {
                                    task.kind = kind;
                                }
                                if !sections.is_empty() {
                                    task.section_ids = sections;
                                }
                                if let Some(status) = parsed_status {
                                    if task.status != status {
                                        task.status = status;
                                        task.status_changed_at = now_ms;
                                    }
                                    super::qa::normalize_task_review_fields(task);
                                }
                                if let Some(body) = body {
                                    task.body = body;
                                }
                                if let Some(model) = model {
                                    task.model = Some(model);
                                }
                                if let Some(why) = why {
                                    task.model_rationale = Some(why);
                                }
                                if let Some(priority) = priority {
                                    task.priority = parse_priority(&priority)?;
                                }
                                if let Some(group) = group {
                                    task.group = Some(group);
                                }
                                if task.personality_provenance
                                    != Some(IdentityProvenance::UserSupplied)
                                {
                                    if let Some(personality) = personality {
                                        task.personality = Some(personality);
                                        task.personality_provenance =
                                            Some(IdentityProvenance::Suggested);
                                    }
                                }
                                task.updated_at = now_ms;
                                task.id.clone()
                            } else {
                                let id = gen_id("task");
                                let personality_provenance =
                                    personality.as_ref().map(|_| IdentityProvenance::Suggested);
                                let task = Task {
                                    id: id.clone(),
                                    name: name.unwrap_or_else(|| handle.clone()),
                                    kind: kind.unwrap_or_else(|| "implement".into()),
                                    model,
                                    model_rationale: why,
                                    acceptance_criteria: vec![],
                                    external_refs: vec![],
                                    test_plan: None,
                                    qa_plan: None,
                                    correctness: None,
                                    status: status
                                        .as_deref()
                                        .map(parse_status)
                                        .transpose()?
                                        .unwrap_or(TaskStatus::Pending),
                                    review_stage: Default::default(),
                                    qa_state: None,
                                    review_round: 0,
                                    review_oracle: None,
                                    review_evidence: vec![],
                                    key: issued_keys.next(),
                                    review_override: None,
                                    review_audit: vec![],
                                    status_label: None,
                                    priority: priority
                                        .as_deref()
                                        .map(parse_priority)
                                        .transpose()?
                                        .unwrap_or_default(),
                                    created_at: now_ms,
                                    updated_at: now_ms,
                                    status_changed_at: now_ms,
                                    handle: Some(handle),
                                    tags: vec![],
                                    section_ids: sections,
                                    body: body.unwrap_or_default(),
                                    role_guidance: None,
                                    executor_role: None,
                                    competition: None,
                                    intent: None,
                                    execution_intent: Default::default(),
                                    parent_task_id: None,
                                    depends_on: vec![],
                                    assigned_tab_id: None,
                                    lane: Default::default(),
                                    taken_by_role: None,
                                    peer: None,
                                    group,
                                    canvas: Default::default(),
                                    milestones: vec![],
                                    execution_attempts: vec![],
                                    pause_reason: None,
                                    interruption: Default::default(),
                                    personality,
                                    personality_provenance,
                                    attachments: vec![],
                                };
                                working.tasks.push(task);
                                id
                            };
                            if let Some(client_id) = client_id {
                                result.ids.insert(client_id, task_id.clone());
                            }
                            if !result.task_ids.iter().any(|id| id == &task_id) {
                                result.task_ids.push(task_id.clone());
                            }
                            result.hints.extend(super::hints::task_create_hints(
                                plan_id,
                                &task_id,
                                !created || !missing_sections,
                                !created || !missing_model,
                                !created || !missing_why,
                                true,
                                true,
                                true,
                            ));
                        }
                        BatchOp::SetTaskBody { task, body } => {
                            let task_id = resolve_ref(&task, &result.ids);
                            let task = find_task_mut(&mut working.tasks, &task_id)
                                .ok_or(PlanError::TaskNotFound)?;
                            task.body = body;
                            task.updated_at = now_ms;
                            if !result.task_ids.iter().any(|id| id == &task.id) {
                                result.task_ids.push(task.id.clone());
                            }
                        }
                        BatchOp::IngestSections { body } => {
                            let ingest =
                                super::ingest::ingest_sections_into_plan(&mut working, &body);
                            for section in ingest.sections {
                                result
                                    .ids
                                    .insert(section.handle.clone(), section.section_id);
                            }
                        }
                        BatchOp::SetTaskSections {
                            client_id,
                            task,
                            sections,
                        } => {
                            let task_id = resolve_ref(&task, &result.ids);
                            let task = find_task_mut(&mut working.tasks, &task_id)
                                .ok_or(PlanError::TaskNotFound)?;
                            task.section_ids = sections;
                            task.updated_at = now_ms;
                            if let Some(client_id) = client_id {
                                result.ids.insert(client_id, task.id.clone());
                            }
                            if !result.task_ids.iter().any(|id| id == &task.id) {
                                result.task_ids.push(task.id.clone());
                            }
                        }
                        BatchOp::SetTaskStatus {
                            client_id,
                            task,
                            status,
                        } => {
                            let task_id = resolve_ref(&task, &result.ids);
                            let parsed = parse_status(&status)?;
                            let task_index = find_task_index(&working.tasks, &task_id)
                                .ok_or(PlanError::TaskNotFound)?;
                            let task_id = working.tasks[task_index].id.clone();
                            super::mutations::validate_task_status_transition(
                                &working,
                                &task_id,
                                &working.tasks[task_index].status,
                                &parsed,
                                None,
                            )?;
                            let task = &mut working.tasks[task_index];
                            if task.status != parsed {
                                task.status = parsed;
                                task.status_changed_at = now_ms;
                            }
                            super::qa::normalize_task_review_fields(task);
                            task.updated_at = now_ms;
                            if let Some(client_id) = client_id {
                                result.ids.insert(client_id, task.id.clone());
                            }
                            if !result.task_ids.iter().any(|id| id == &task.id) {
                                result.task_ids.push(task.id.clone());
                            }
                        }
                        BatchOp::SetGoal {
                            client_id,
                            vector,
                            vision,
                            constraints,
                            postulates,
                        } => {
                            working.goal = Some(Goal {
                                vector,
                                vision,
                                constraints,
                                postulates,
                            });
                            if let Some(client_id) = client_id {
                                result.ids.insert(client_id, working.id.clone());
                            }
                        }
                        BatchOp::AddMilestone {
                            client_id,
                            task,
                            kind,
                            text,
                            from,
                            key,
                            links,
                        } => {
                            let task_id = resolve_ref(&task, &result.ids);
                            ensure_kind(&mut working.milestone_kinds, &kind);
                            let key_link = milestone_key(key.as_deref(), request.key.as_deref());
                            let task = find_task_mut(&mut working.tasks, &task_id)
                                .ok_or(PlanError::TaskNotFound)?;
                            let existing = key_link.as_ref().and_then(|key_link| {
                                task.milestones
                                    .iter()
                                    .find(|ms| {
                                        ms.kind == kind
                                            && ms.links.iter().any(|link| link == key_link)
                                    })
                                    .map(|ms| ms.id.clone())
                            });
                            let milestone_id = if let Some(existing) = existing {
                                existing
                            } else {
                                let id = gen_id("ms");
                                let mut all_links = links
                                    .into_iter()
                                    .map(|link| resolve_ref(&link, &result.ids))
                                    .collect::<Vec<_>>();
                                if let Some(key_link) = key_link {
                                    all_links.push(key_link);
                                }
                                task.milestones.push(Milestone {
                                    id: id.clone(),
                                    from: from.unwrap_or_else(|| AgentRef::from_tab("unknown")),
                                    kind,
                                    text,
                                    ts: now_ms,
                                    links: all_links,
                                    loop_id: None,
                                    iteration: None,
                                    canvas: Default::default(),
                                    attachments: vec![],
                                });
                                task.updated_at = now_ms;
                                id
                            };
                            if let Some(client_id) = client_id {
                                result.ids.insert(client_id, milestone_id.clone());
                            }
                            if !result.milestone_ids.iter().any(|id| id == &milestone_id) {
                                result.milestone_ids.push(milestone_id);
                            }
                        }
                        BatchOp::AddElement {
                            client_id,
                            kind,
                            content,
                            link,
                            links,
                            x,
                            y,
                        } => {
                            let id = gen_id("el");
                            let mut all_links = links
                                .into_iter()
                                .map(|link| resolve_ref(&link, &result.ids))
                                .collect::<Vec<_>>();
                            if let Some(link) = link {
                                all_links.push(resolve_ref(&link, &result.ids));
                            }
                            working.elements.push(CanvasElement {
                                id: id.clone(),
                                kind,
                                canvas: ElementCanvas {
                                    x,
                                    y,
                                    collapsed: false,
                                },
                                content,
                                links: all_links,
                            });
                            if let Some(client_id) = client_id {
                                result.ids.insert(client_id, id.clone());
                            }
                            result.element_ids.push(id);
                        }
                        BatchOp::AddHandoff {
                            client_id,
                            mode,
                            mut content,
                            body,
                            task,
                            from,
                            to,
                        } => {
                            let id = gen_id("el");
                            let seq = next_handoff_seq(&working);
                            let body_path = super::body_files::handoff_path(&working, &id)
                                .map_err(|e| PlanError::Io(e.to_string()))?;
                            let relative_body_path = body_path
                                .strip_prefix(std::path::Path::new(&working.project_path))
                                .map(|path| path.display().to_string())
                                .unwrap_or_else(|_| body_path.display().to_string());
                            pending_markdown_writes.push((body_path, body));
                            if !content.is_object() {
                                content = serde_json::json!({});
                            }
                            if let Some(obj) = content.as_object_mut() {
                                obj.insert("seq".into(), Value::from(seq));
                                obj.insert(
                                    "mode".into(),
                                    Value::from(mode.unwrap_or_else(|| "record".into())),
                                );
                                obj.insert("plan_id".into(), Value::from(working.id.clone()));
                                obj.insert("plan_title".into(), Value::from(working.title.clone()));
                                obj.insert("ts".into(), Value::from(now_ms));
                                obj.insert("body_path".into(), Value::from(relative_body_path));
                                obj.entry("reseed_state")
                                    .or_insert_with(|| Value::from("created"));
                            }
                            let mut links = vec![format!("plan:{}", working.id)];
                            if seq > 1 {
                                links.push(format!("handoff:{}", seq - 1));
                            }
                            if let Some(task) = task.as_ref() {
                                links.push(resolve_ref(task, &result.ids));
                            }
                            working.elements.push(CanvasElement {
                                id: id.clone(),
                                kind: "handoff".into(),
                                canvas: ElementCanvas {
                                    x: None,
                                    y: None,
                                    collapsed: false,
                                },
                                content,
                                links,
                            });
                            if let Some(task) = task {
                                let task_id = resolve_ref(&task, &result.ids);
                                ensure_kind(&mut working.milestone_kinds, "handoff");
                                let task = find_task_mut(&mut working.tasks, &task_id)
                                    .ok_or(PlanError::TaskNotFound)?;
                                let milestone_id = gen_id("ms");
                                task.milestones.push(Milestone {
                                    id: milestone_id.clone(),
                                    from: from
                                        .clone()
                                        .unwrap_or_else(|| AgentRef::from_tab("unknown")),
                                    kind: "handoff".into(),
                                    text: format!("handoff #{}", seq),
                                    ts: now_ms,
                                    links: vec![id.clone()],
                                    loop_id: None,
                                    iteration: None,
                                    canvas: Default::default(),
                                    attachments: vec![],
                                });
                                task.updated_at = now_ms;
                                result.milestone_ids.push(milestone_id);
                            }
                            let from = from.unwrap_or_else(|| working.owner.clone());
                            let to = to.unwrap_or_else(|| working.owner.clone());
                            working.owner_history.push(TransferRecord {
                                from,
                                to,
                                ts: now_ms,
                                reason: format!("handoff #{}", seq),
                            });
                            if let Some(client_id) = client_id {
                                result.ids.insert(client_id, id.clone());
                            }
                            result.element_ids.push(id);
                        }
                        BatchOp::Link { to } => {
                            let to = resolve_ref(&to, &result.ids);
                            if !working.links.iter().any(|link| link == &to) {
                                working.links.push(to);
                            }
                        }
                        BatchOp::UpsertSpec {
                            client_id,
                            spec_id,
                            title,
                        } => {
                            let spec_id = spec_id.map(|id| resolve_ref(&id, &result.ids));
                            let id = if let Some(spec_id) = spec_id {
                                let spec = staged_spec(&mut staged_specs, self, &spec_id)?;
                                spec.title = title;
                                spec.updated_at = now_ms;
                                spec_id
                            } else {
                                let mut spec = Spec::new(title, working.project_path.clone());
                                spec.created_at = now_ms;
                                spec.updated_at = now_ms;
                                let id = spec.id.clone();
                                staged_specs.insert(id.clone(), spec);
                                id
                            };
                            if let Some(client_id) = client_id {
                                result.ids.insert(client_id, id.clone());
                            }
                            if !result.spec_ids.iter().any(|existing| existing == &id) {
                                result.spec_ids.push(id);
                            }
                        }
                        BatchOp::UpsertSpecSection {
                            client_id,
                            spec,
                            section_id,
                            title,
                            body,
                        } => {
                            let spec_id = resolve_ref(&spec, &result.ids);
                            let section_id = section_id.map(|id| resolve_ref(&id, &result.ids));
                            let id = if let Some(section_id) = section_id {
                                let spec = staged_spec(&mut staged_specs, self, &spec_id)?;
                                let section = find_section_mut(&mut spec.sections, &section_id)
                                    .ok_or(PlanError::NotFound)?;
                                section.title = title;
                                if let Some(body) = body {
                                    section.body = body;
                                }
                                spec.updated_at = now_ms;
                                section_id
                            } else {
                                let spec = staged_spec(&mut staged_specs, self, &spec_id)?;
                                let id = gen_id("sec");
                                spec.sections.push(Section {
                                    id: id.clone(),
                                    title,
                                    handle: None,
                                    tags: vec![],
                                    body: body.unwrap_or_default(),
                                    subsections: vec![],
                                    canvas: Default::default(),
                                    attachments: vec![],
                                });
                                spec.updated_at = now_ms;
                                id
                            };
                            if let Some(client_id) = client_id {
                                result.ids.insert(client_id, id.clone());
                            }
                            if !result
                                .spec_section_ids
                                .iter()
                                .any(|existing| existing == &id)
                            {
                                result.spec_section_ids.push(id);
                            }
                        }
                        BatchOp::SetSpecSectionBody {
                            spec,
                            section,
                            body,
                        } => {
                            let spec_id = resolve_ref(&spec, &result.ids);
                            let section_id = resolve_ref(&section, &result.ids);
                            let spec = staged_spec(&mut staged_specs, self, &spec_id)?;
                            let section = find_section_mut(&mut spec.sections, &section_id)
                                .ok_or(PlanError::NotFound)?;
                            section.body = body;
                            spec.updated_at = now_ms;
                            if !result
                                .spec_section_ids
                                .iter()
                                .any(|existing| existing == &section_id)
                            {
                                result.spec_section_ids.push(section_id);
                            }
                        }
                        BatchOp::LinkSpecSection {
                            spec,
                            spec_section,
                            plan_section,
                        } => {
                            let spec_id = resolve_ref(&spec, &result.ids);
                            let spec_section_id = resolve_ref(&spec_section, &result.ids);
                            let plan_section_id = resolve_ref(&plan_section, &result.ids);
                            if !working.spec_refs.iter().any(|id| id == &spec_id) {
                                working.spec_refs.push(spec_id.clone());
                            }
                            if working.primary_spec_id.is_none() {
                                working.primary_spec_id = Some(spec_id.clone());
                            }
                            let link = crate::plan::SpecLink {
                                spec_id,
                                spec_section_id,
                                plan_section_id,
                            };
                            if !working.spec_links.iter().any(|existing| existing == &link) {
                                working.spec_links.push(link);
                            }
                        }
                        BatchOp::UnlinkSpecSection {
                            spec,
                            spec_section,
                            plan_section,
                        } => {
                            let spec_id = resolve_ref(&spec, &result.ids);
                            let spec_section = spec_section.map(|id| resolve_ref(&id, &result.ids));
                            let plan_section = plan_section.map(|id| resolve_ref(&id, &result.ids));
                            working.spec_links.retain(|link| {
                                !(link.spec_id == spec_id
                                    && spec_section
                                        .as_deref()
                                        .is_none_or(|sid| link.spec_section_id == sid)
                                    && plan_section
                                        .as_deref()
                                        .is_none_or(|sid| link.plan_section_id == sid))
                            });
                        }
                        BatchOp::CreatePlanFromSpec {
                            client_id,
                            spec,
                            title,
                        } => {
                            let spec_id = resolve_ref(&spec, &result.ids);
                            if !staged_specs.contains_key(&spec_id)
                                && self.get_spec(&spec_id).is_none()
                            {
                                return Err(PlanError::NotFound);
                            }
                            let owner = working.owner.clone();
                            let mut plan = Plan::new(title, working.project_path.clone(), owner);
                            plan.status = PlanStatus::Archived;
                            if plan.id == working.id {
                                return Err(PlanError::Io(
                                    "derived plan id must differ from source plan id".into(),
                                ));
                            }
                            plan.milestone_kinds = working.milestone_kinds.clone();
                            plan.created_at = now_ms;
                            plan.updated_at = now_ms;
                            plan.spec_refs.push(spec_id.clone());
                            plan.primary_spec_id = Some(spec_id);
                            let plan_id = plan.id.clone();
                            staged_plans.push(plan);
                            if let Some(client_id) = client_id {
                                result.ids.insert(client_id, plan_id);
                            }
                        }
                    }
                }
                if let Some(hint) = super::hints::maybe_orchestration_reminder() {
                    result.hints.push(hint);
                }
                *plan = working;
                Ok((result, pending_markdown_writes, staged_specs, staged_plans))
            })?;
        for (path, body) in pending_markdown_writes {
            super::body_files::write_markdown_atomic(&path, &body)
                .map_err(|e| PlanError::Io(e.to_string()))?;
        }
        for spec in staged_specs.into_values() {
            self.commit_loaded_spec(spec)?;
        }
        for plan in staged_plans {
            self.commit_loaded_plan(plan)?;
        }
        Ok(result)
    }
}

impl From<NewTask> for BatchOp {
    fn from(task: NewTask) -> Self {
        BatchOp::UpsertTask {
            client_id: None,
            handle: task.handle.unwrap_or(task.name.clone()),
            name: Some(task.name),
            kind: Some(task.kind),
            sections: task.section_ids,
            status: None,
            body: Some(task.body),
            model: task.model,
            why: task.model_rationale,
            priority: task.priority.map(|p| {
                serde_json::to_value(p)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_else(|| "p2".into())
            }),
            group: task.group,
            personality: task.personality,
            personality_provenance: task.personality_provenance,
        }
    }
}

#[cfg(test)]
mod tests;
