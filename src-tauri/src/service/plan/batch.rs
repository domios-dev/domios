use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{NewTask, PlanError, PlanStore};
use crate::plan::{
    AgentRef, CanvasElement, ElementCanvas, Milestone, MilestoneKind, Task, TaskPriority,
    TaskStatus, TransferRecord,
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
    },
    SetTaskBody {
        task: String,
        #[serde(alias = "body_md")]
        body: String,
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
}

impl Default for BatchResult {
    fn default() -> Self {
        Self {
            ok: true,
            ids: HashMap::new(),
            task_ids: Vec::new(),
            milestone_ids: Vec::new(),
            element_ids: Vec::new(),
        }
    }
}

fn gen_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
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
    tasks
        .iter_mut()
        .find(|task| task.id == selector || task.handle.as_deref() == Some(selector))
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

impl PlanStore {
    pub fn apply_batch(
        &self,
        plan_id: &str,
        request: BatchRequest,
        now_ms: u64,
    ) -> Result<BatchResult, PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let mut working = plan.clone();
            let mut result = BatchResult::default();
            let mut pending_markdown_writes: Vec<(PathBuf, String)> = Vec::new();
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
                    } => {
                        let task_id = if let Some(task) = working.tasks.iter_mut().find(|task| {
                            task.id == handle || task.handle.as_deref() == Some(handle.as_str())
                        }) {
                            if let Some(name) = name {
                                task.name = name;
                            }
                            if let Some(kind) = kind {
                                task.kind = kind;
                            }
                            if !sections.is_empty() {
                                task.section_ids = sections;
                            }
                            if let Some(status) = status {
                                let parsed = parse_status(&status)?;
                                if task.status != parsed {
                                    task.status = parsed;
                                    task.status_changed_at = now_ms;
                                }
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
                            task.updated_at = now_ms;
                            task.id.clone()
                        } else {
                            let id = gen_id("task");
                            let task = Task {
                                id: id.clone(),
                                name: name.unwrap_or_else(|| handle.clone()),
                                kind: kind.unwrap_or_else(|| "implement".into()),
                                model,
                                model_rationale: why,
                                status: status
                                    .as_deref()
                                    .map(parse_status)
                                    .transpose()?
                                    .unwrap_or(TaskStatus::Pending),
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
                                intent: None,
                                parent_task_id: None,
                                assigned_pane_id: None,
                                taken_by_role: None,
                                peer: None,
                                group,
                                canvas: Default::default(),
                                milestones: vec![],
                            };
                            working.tasks.push(task);
                            id
                        };
                        if let Some(client_id) = client_id {
                            result.ids.insert(client_id, task_id.clone());
                        }
                        if !result.task_ids.iter().any(|id| id == &task_id) {
                            result.task_ids.push(task_id);
                        }
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
                                    ms.kind == kind && ms.links.iter().any(|link| link == key_link)
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
                                from: from.unwrap_or_else(|| AgentRef::from_pane("unknown")),
                                kind,
                                text,
                                ts: now_ms,
                                links: all_links,
                                loop_id: None,
                                iteration: None,
                                canvas: Default::default(),
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
                        pending_markdown_writes.push((body_path, body));
                        let relative_body_path =
                            format!(".codeterm/plans/{}/handoffs/{}.md", working.id, id);
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
                                    .unwrap_or_else(|| AgentRef::from_pane("unknown")),
                                kind: "handoff".into(),
                                text: format!("handoff #{}", seq),
                                ts: now_ms,
                                links: vec![id.clone()],
                                loop_id: None,
                                iteration: None,
                                canvas: Default::default(),
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
                }
            }
            for (path, body) in pending_markdown_writes {
                super::body_files::write_markdown_atomic(&path, &body)
                    .map_err(|e| PlanError::Io(e.to_string()))?;
            }
            *plan = working;
            Ok(result)
        })
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan};
    use std::path::PathBuf;

    fn store_with_plan() -> (PlanStore, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::mem::forget(dir);
        let store = PlanStore::new_for_test(&root);
        let mut plan = Plan::new(
            "Batch".into(),
            root.display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_batch".into();
        store.save(&mut plan, 1).unwrap();
        (store, plan.id)
    }

    fn store_with_plan_root() -> (PathBuf, PlanStore, String) {
        let root = tempfile::tempdir().unwrap().keep();
        let store = PlanStore::new_for_test(&root);
        let mut plan = Plan::new(
            "Batch".into(),
            root.display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_batch".into();
        store.save(&mut plan, 1).unwrap();
        (root, store, plan.id)
    }

    #[test]
    fn batch_resolves_refs_and_writes_in_one_plan_mutation() {
        let (store, plan_id) = store_with_plan();
        let result = store
            .apply_batch(
                &plan_id,
                BatchRequest {
                    key: Some("k1".into()),
                    ops: vec![
                        BatchOp::UpsertTask {
                            client_id: Some("t".into()),
                            handle: "handoff-cli".into(),
                            name: Some("Handoff CLI".into()),
                            kind: None,
                            sections: vec![],
                            status: Some("in_progress".into()),
                            body: None,
                            model: None,
                            why: None,
                            priority: None,
                            group: None,
                        },
                        BatchOp::AddMilestone {
                            client_id: Some("m".into()),
                            task: "$t".into(),
                            kind: "decision".into(),
                            text: "ship batch first".into(),
                            from: None,
                            key: None,
                            links: vec![],
                        },
                        BatchOp::AddElement {
                            client_id: Some("e".into()),
                            kind: "note".into(),
                            content: serde_json::json!({"text":"x"}),
                            link: Some("$t".into()),
                            links: vec![],
                            x: None,
                            y: None,
                        },
                    ],
                },
                2,
            )
            .unwrap();
        assert!(result.ids.contains_key("t"));
        assert!(result.ids.contains_key("m"));
        assert!(result.ids.contains_key("e"));
        let plan = store.get(&plan_id).unwrap();
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].milestones.len(), 1);
        assert_eq!(plan.elements[0].links, vec![plan.tasks[0].id.clone()]);
    }

    #[test]
    fn batch_is_atomic_when_later_op_fails() {
        let (store, plan_id) = store_with_plan();
        let err = store
            .apply_batch(
                &plan_id,
                BatchRequest {
                    key: None,
                    ops: vec![
                        BatchOp::UpsertTask {
                            client_id: Some("t".into()),
                            handle: "created".into(),
                            name: None,
                            kind: None,
                            sections: vec![],
                            status: None,
                            body: None,
                            model: None,
                            why: None,
                            priority: None,
                            group: None,
                        },
                        BatchOp::SetTaskBody {
                            task: "missing".into(),
                            body: "nope".into(),
                        },
                    ],
                },
                2,
            )
            .unwrap_err();
        assert!(matches!(err, PlanError::TaskNotFound));
        assert!(store.get(&plan_id).unwrap().tasks.is_empty());
    }

    #[test]
    fn batch_handoff_sidecar_is_not_written_when_later_op_fails() {
        let (root, store, plan_id) = store_with_plan_root();
        let err = store
            .apply_batch(
                &plan_id,
                BatchRequest {
                    key: None,
                    ops: vec![
                        BatchOp::AddHandoff {
                            client_id: Some("h".into()),
                            mode: None,
                            content: serde_json::json!({"goal":"rollback"}),
                            body: "must not remain".into(),
                            task: None,
                            from: None,
                            to: None,
                        },
                        BatchOp::SetTaskBody {
                            task: "missing".into(),
                            body: "fail".into(),
                        },
                    ],
                },
                2,
            )
            .unwrap_err();
        assert!(matches!(err, PlanError::TaskNotFound));
        assert!(store.get(&plan_id).unwrap().elements.is_empty());
        let handoffs_dir = root
            .join(".codeterm")
            .join("plans")
            .join(&plan_id)
            .join("handoffs");
        let leftover = handoffs_dir
            .read_dir()
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        assert!(!leftover);
    }

    #[test]
    fn batch_upsert_and_milestone_key_are_idempotent() {
        let (store, plan_id) = store_with_plan();
        let request = BatchRequest {
            key: Some("stable".into()),
            ops: vec![
                BatchOp::UpsertTask {
                    client_id: Some("t".into()),
                    handle: "same".into(),
                    name: Some("Same".into()),
                    kind: None,
                    sections: vec![],
                    status: None,
                    body: None,
                    model: None,
                    why: None,
                    priority: None,
                    group: None,
                },
                BatchOp::AddMilestone {
                    client_id: Some("m".into()),
                    task: "$t".into(),
                    kind: "testing".into(),
                    text: "ran".into(),
                    from: None,
                    key: None,
                    links: vec![],
                },
            ],
        };
        let first = store.apply_batch(&plan_id, request.clone(), 2).unwrap();
        let second = store.apply_batch(&plan_id, request, 3).unwrap();
        assert_eq!(first.task_ids, second.task_ids);
        assert_eq!(first.milestone_ids, second.milestone_ids);
        let plan = store.get(&plan_id).unwrap();
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].milestones.len(), 1);
    }
}
