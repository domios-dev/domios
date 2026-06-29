use super::{NewTask, OpenSubtask, PlanError, PlanStore};
use crate::plan::{
    AgentRef, CanvasElement, Goal, Milestone, MilestoneKind, OrchestratorRef, Plan, PlanStatus,
    Section, TagFacet, Task, TaskGroup, TaskLaneMetadata, TaskPriority, TaskStatus, TransferRecord,
};

fn gen_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

fn normalize_lane_metadata(mut lane: TaskLaneMetadata) -> TaskLaneMetadata {
    lane.pane_id = non_empty(lane.pane_id);
    lane.workspace_id = non_empty(lane.workspace_id);
    lane.workspace_name = non_empty(lane.workspace_name);
    lane.worktree_path = non_empty(lane.worktree_path);
    lane.branch = non_empty(lane.branch);
    lane.base = non_empty(lane.base);
    lane.destination = non_empty(lane.destination);
    lane.cleanup_status = non_empty(lane.cleanup_status);
    lane.cleanup_hints.retain(|hint| !hint.trim().is_empty());
    lane
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn apply_lane_update(lane: &mut TaskLaneMetadata, update: super::TaskLaneUpdate) {
    if let Some(value) = update.pane_id {
        lane.pane_id = value;
    }
    if let Some(value) = update.workspace_id {
        lane.workspace_id = value;
    }
    if let Some(value) = update.workspace_name {
        lane.workspace_name = value;
    }
    if let Some(value) = update.worktree_path {
        lane.worktree_path = value;
    }
    if let Some(value) = update.branch {
        lane.branch = value;
    }
    if let Some(value) = update.base {
        lane.base = value;
    }
    if let Some(value) = update.destination {
        lane.destination = value;
    }
    if let Some(value) = update.cleanup_status {
        lane.cleanup_status = value;
    }
    if let Some(value) = update.cleanup_hints {
        lane.cleanup_hints = value;
    }
}

pub(super) fn push_subsection(
    sections: &mut Vec<Section>,
    parent_id: &str,
    child: Section,
) -> Option<Section> {
    for s in sections.iter_mut() {
        if s.id == parent_id {
            s.subsections.push(child);
            return None;
        }
    }
    let mut child = child;
    for s in sections.iter_mut() {
        match push_subsection(&mut s.subsections, parent_id, child) {
            None => return None,
            Some(c) => child = c,
        }
    }
    Some(child)
}

pub(super) fn update_section_fields(
    sections: &mut [Section],
    id: &str,
    title: Option<&str>,
    body: Option<&str>,
) -> bool {
    for s in sections.iter_mut() {
        if s.id == id || s.handle.as_deref() == Some(id) {
            if let Some(t) = title {
                s.title = t.into();
                s.handle = None;
            }
            if let Some(b) = body {
                s.body = b.into();
            }
            return true;
        }
        if update_section_fields(&mut s.subsections, id, title, body) {
            return true;
        }
    }
    false
}

pub(super) fn update_section_tags(
    sections: &mut [Section],
    id: &str,
    tags: &[String],
    primary: Option<&str>,
    handle: Option<&str>,
) -> bool {
    for s in sections.iter_mut() {
        if s.id == id || s.handle.as_deref() == Some(id) {
            super::tag_model::apply_tags(&mut s.tags, tags, primary);
            if !tags.is_empty() || primary.is_some() {
                s.handle = None;
            }
            if let Some(handle) = handle {
                s.handle = Some(handle.into());
            }
            return true;
        }
        if update_section_tags(&mut s.subsections, id, tags, primary, handle) {
            return true;
        }
    }
    false
}

pub(super) fn rm_section_tree(sections: &mut Vec<Section>, id: &str) -> bool {
    if let Some(pos) = sections.iter().position(|s| s.id == id) {
        sections.remove(pos);
        return true;
    }
    sections
        .iter_mut()
        .any(|s| rm_section_tree(&mut s.subsections, id))
}

pub(super) fn has_section(sections: &[Section], id: &str) -> bool {
    sections.iter().any(|section| {
        section.id == id
            || section.handle.as_deref() == Some(id)
            || has_section(&section.subsections, id)
    })
}

fn set_section_canvas(sections: &mut [Section], id: &str, x: Option<f64>, y: Option<f64>) -> bool {
    for s in sections.iter_mut() {
        if s.id == id {
            s.canvas.x = x;
            s.canvas.y = y;
            return true;
        }
        if set_section_canvas(&mut s.subsections, id, x, y) {
            return true;
        }
    }
    false
}

fn parse_task_status(raw: &str) -> Result<TaskStatus, PlanError> {
    serde_json::from_value(serde_json::Value::String(raw.into())).map_err(|_| PlanError::BadStatus)
}

fn parse_task_priority(raw: &str) -> Result<TaskPriority, PlanError> {
    serde_json::from_value(serde_json::Value::String(raw.into()))
        .map_err(|_| PlanError::BadPriority)
}

fn is_terminal_task_status(status: &TaskStatus) -> bool {
    matches!(status, TaskStatus::Done | TaskStatus::Cancelled)
}

fn open_subtasks(plan: &Plan, parent_task_id: &str) -> Vec<OpenSubtask> {
    plan.tasks
        .iter()
        .filter(|task| task.parent_task_id.as_deref() == Some(parent_task_id))
        .filter(|task| !is_terminal_task_status(&task.status))
        .map(|task| OpenSubtask {
            id: task.id.clone(),
            name: task.name.clone(),
            status: task.status.clone(),
            handle: task.handle.clone(),
        })
        .collect()
}

pub(super) fn reject_done_with_open_subtasks(plan: &Plan, task_id: &str) -> Result<(), PlanError> {
    let subtasks = open_subtasks(plan, task_id);
    if subtasks.is_empty() {
        Ok(())
    } else {
        Err(PlanError::OpenSubtasks { subtasks })
    }
}

pub(super) fn validate_task_status_transition(
    plan: &Plan,
    task_id: &str,
    current: &TaskStatus,
    next: &TaskStatus,
) -> Result<(), PlanError> {
    if current == next {
        return Ok(());
    }
    if matches!(next, TaskStatus::Done) {
        reject_done_with_open_subtasks(plan, task_id)?;
    }
    if matches!((current, next), (TaskStatus::InReview, TaskStatus::Done)) {
        return Err(PlanError::TransitionDenied {
            from: current.clone(),
            to: next.clone(),
        });
    }
    Ok(())
}

impl PlanStore {
    pub fn create_plan(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
        // Canonicalize to the git toplevel so storage + index + queries agree on
        // one root (not a sub-path like src-tauri/ or the daemon cwd).
        let project_path =
            crate::service::plan::paths::project_root(std::path::Path::new(&project_path))
                .display()
                .to_string();
        let mut plan = Plan::new(title, project_path, owner);
        plan.milestone_kinds = kinds;
        self.save(&mut plan, now_ms)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        Ok(plan)
    }

    pub fn update_plan(
        &self,
        plan_id: &str,
        title: Option<&str>,
        status: Option<PlanStatus>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            if let Some(t) = title {
                p.title = t.into();
            }
            if let Some(s) = status {
                p.status = s;
            }
            Ok(())
        })
    }

    pub fn set_kinds(
        &self,
        plan_id: &str,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            p.milestone_kinds = kinds;
            Ok(())
        })
    }

    pub fn set_task_groups(
        &self,
        plan_id: &str,
        groups: Vec<TaskGroup>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            p.task_groups = groups;
            Ok(())
        })
    }

    pub fn set_tag_facets(
        &self,
        plan_id: &str,
        facets: Vec<TagFacet>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            p.tag_facets = facets;
            Ok(())
        })
    }

    pub fn set_goal(&self, plan_id: &str, goal: Goal, now_ms: u64) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            p.goal = Some(goal);
            Ok(())
        })
    }

    pub fn link_plan(&self, plan_id: &str, to: &str, now_ms: u64) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            if !p.links.iter().any(|l| l == to) {
                p.links.push(to.into());
            }
            Ok(())
        })
    }

    pub fn unlink_plan(&self, plan_id: &str, to: &str, now_ms: u64) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            p.links.retain(|l| l != to);
            Ok(())
        })
    }

    pub(super) fn with_plan<T, F>(&self, id: &str, now_ms: u64, f: F) -> Result<T, PlanError>
    where
        F: FnOnce(&mut Plan) -> Result<T, PlanError>,
    {
        let arc = self
            .plans
            .read()
            .get(id)
            .cloned()
            .ok_or(PlanError::NotFound)?;
        let mut guard = arc.lock();
        let out = f(&mut guard)?;
        guard.updated_at = now_ms;
        super::tag_model::normalize_plan(&mut guard);
        let path = Self::write_atomic(&guard).map_err(|e| PlanError::Io(e.to_string()))?;
        self.note_plan_write(&guard, &path);
        self.mirror_plan_index(&guard)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        let entry = Self::index_entry(&guard, path);
        self.index.write().insert(guard.id.clone(), entry);
        self.reset_bound_panes(&guard);
        Ok(out)
    }

    pub fn owner_of(&self, id: &str) -> Option<OrchestratorRef> {
        self.plans.read().get(id).map(|p| p.lock().owner.clone())
    }

    pub fn add_section(
        &self,
        plan_id: &str,
        title: &str,
        body: &str,
        parent: Option<&str>,
        tags: &[String],
        primary: Option<&str>,
        handle: Option<&str>,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let sid = gen_id("sec");
        let mut section_tags = tags.to_vec();
        super::tag_model::apply_tags(&mut section_tags, &[], primary);
        let section = Section {
            id: sid.clone(),
            title: title.into(),
            handle: handle.map(str::to_string),
            tags: section_tags,
            body: body.into(),
            subsections: vec![],
            canvas: Default::default(),
        };
        self.with_plan(plan_id, now_ms, |p| match parent {
            None => {
                p.sections.push(section);
                Ok(())
            }
            Some(pid) => match push_subsection(&mut p.sections, pid, section) {
                None => Ok(()),
                Some(_) => Err(PlanError::NotFound),
            },
        })?;
        Ok(sid)
    }

    pub fn add_task(&self, plan_id: &str, new: NewTask, now_ms: u64) -> Result<String, PlanError> {
        let tid = gen_id("task");
        let assigned_pane_id = new
            .assigned_pane_id
            .clone()
            .or_else(|| new.lane.pane_id.clone());
        let mut lane = normalize_lane_metadata(new.lane);
        if lane.pane_id.is_none() {
            lane.pane_id = assigned_pane_id.clone();
        }
        let mut task = Task {
            id: tid.clone(),
            name: new.name,
            kind: new.kind,
            model: new.model,
            model_rationale: new.model_rationale,
            acceptance_criteria: new.acceptance_criteria,
            test_plan: new.test_plan,
            correctness: new.correctness,
            status: TaskStatus::Pending,
            status_label: None,
            priority: new.priority.unwrap_or_default(),
            created_at: now_ms,
            updated_at: now_ms,
            status_changed_at: now_ms,
            handle: new.handle,
            tags: new.tags,
            section_ids: new.section_ids,
            body: new.body,
            intent: new.intent,
            parent_task_id: new.parent_task_id,
            assigned_pane_id,
            lane,
            taken_by_role: None,
            peer: None,
            group: new.group,
            canvas: Default::default(),
            milestones: vec![],
        };
        self.with_plan(plan_id, now_ms, |p| {
            if task.section_ids.is_empty() {
                task.section_ids = super::section_links::auto_section_ids_for_task(p, &task);
            }
            p.tasks.push(task);
            Ok(())
        })?;
        Ok(tid)
    }

    pub fn close_task(
        &self,
        plan_id: &str,
        task_id: &str,
        result: Option<&str>,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task_index = p
                .tasks
                .iter()
                .position(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            let task_id = p.tasks[task_index].id.clone();
            reject_done_with_open_subtasks(p, &task_id)?;
            let task = &mut p.tasks[task_index];
            if task.status != TaskStatus::Done {
                task.status_changed_at = now_ms;
            }
            task.status = TaskStatus::Done;
            task.updated_at = now_ms;
            if let Some(text) = result {
                task.milestones.push(Milestone {
                    id: gen_id("ms"),
                    from,
                    kind: "status_update".into(),
                    text: text.into(),
                    ts: now_ms,
                    links: vec![],
                    loop_id: None,
                    iteration: None,
                    canvas: Default::default(),
                });
            }
            Ok(())
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_milestone(
        &self,
        plan_id: &str,
        task_id: &str,
        kind: &str,
        text: &str,
        from: AgentRef,
        add_kind: bool,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let mid = gen_id("ms");
        self.with_plan(plan_id, now_ms, |p| {
            let declared = p.milestone_kinds.iter().any(|k| k.key == kind);
            if !declared {
                if add_kind {
                    p.milestone_kinds.push(MilestoneKind {
                        key: kind.into(),
                        label: kind.into(),
                        color: None,
                    });
                } else {
                    return Err(PlanError::UndeclaredKind {
                        declared: p.milestone_kinds.iter().map(|k| k.key.clone()).collect(),
                    });
                }
            }
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or(PlanError::TaskNotFound)?;
            task.milestones.push(Milestone {
                id: mid.clone(),
                from,
                kind: kind.into(),
                text: text.into(),
                ts: now_ms,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
            });
            Ok(())
        })?;
        Ok(mid)
    }

    pub fn archive(&self, plan_id: &str, now_ms: u64) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            p.status = PlanStatus::Archived;
            Ok(())
        })
    }

    pub fn unarchive(&self, plan_id: &str, now_ms: u64) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            p.status = PlanStatus::Active;
            Ok(())
        })
    }

    pub fn transfer(
        &self,
        plan_id: &str,
        to: OrchestratorRef,
        reason: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            p.owner_history.push(TransferRecord {
                from: p.owner.clone(),
                to: to.clone(),
                ts: now_ms,
                reason: reason.into(),
            });
            p.owner = to;
            Ok(())
        })
    }

    pub fn disown(&self, plan_id: &str, now_ms: u64) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            p.owner_history.push(TransferRecord {
                from: p.owner.clone(),
                to: OrchestratorRef::default(),
                ts: now_ms,
                reason: "disown".into(),
            });
            p.owner = OrchestratorRef::default();
            Ok(())
        })
    }

    pub fn claim(
        &self,
        plan_id: &str,
        new_owner: OrchestratorRef,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            p.owner_history.push(TransferRecord {
                from: p.owner.clone(),
                to: new_owner.clone(),
                ts: now_ms,
                reason: "claim".into(),
            });
            p.owner = new_owner;
            Ok(())
        })
    }

    pub fn update_section(
        &self,
        plan_id: &str,
        section_id: &str,
        title: Option<&str>,
        body: Option<&str>,
        tags: &[String],
        primary: Option<&str>,
        handle: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            if update_section_fields(&mut p.sections, section_id, title, body) {
                update_section_tags(&mut p.sections, section_id, tags, primary, handle);
                Ok(())
            } else {
                Err(PlanError::NotFound)
            }
        })
    }

    pub fn rm_section(
        &self,
        plan_id: &str,
        section_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            if rm_section_tree(&mut p.sections, section_id) {
                Ok(())
            } else {
                Err(PlanError::NotFound)
            }
        })
    }

    pub fn update_task(
        &self,
        plan_id: &str,
        task_id: &str,
        name: Option<&str>,
        kind: Option<&str>,
        model: Option<Option<&str>>,
        model_rationale: Option<Option<&str>>,
        status: Option<&str>,
        status_label: Option<Option<&str>>,
        priority: Option<&str>,
        body: Option<&str>,
        intent: Option<Option<&str>>,
        section_ids: Option<Vec<String>>,
        parent_task_id: Option<Option<&str>>,
        group: Option<Option<&str>>,
        tags: &[String],
        primary: Option<&str>,
        handle: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task_index = p
                .tasks
                .iter()
                .position(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            let task_id = p.tasks[task_index].id.clone();
            let parsed_status = status.map(parse_task_status).transpose()?;
            if let Some(next) = &parsed_status {
                validate_task_status_transition(p, &task_id, &p.tasks[task_index].status, next)?;
            }
            let task = &mut p.tasks[task_index];
            if let Some(n) = name {
                task.name = n.into();
                task.handle = None;
            }
            if let Some(k) = kind {
                task.kind = k.into();
            }
            if let Some(m) = model {
                task.model = m.map(|s| s.into());
            }
            if let Some(w) = model_rationale {
                task.model_rationale = w.map(|s| s.into());
            }
            if let Some(status) = parsed_status {
                if task.status != status {
                    task.status = status;
                    task.status_changed_at = now_ms;
                }
            }
            if let Some(priority) = priority {
                task.priority = parse_task_priority(priority)?;
            }
            if let Some(l) = status_label {
                task.status_label = l.map(|s| s.into());
            }
            if let Some(b) = body {
                task.body = b.into();
            }
            if let Some(i) = intent {
                task.intent = i.map(|s| s.into());
            }
            if let Some(ids) = section_ids {
                task.section_ids = ids;
            }
            if let Some(parent) = parent_task_id {
                task.parent_task_id = parent.map(|s| s.into());
            }
            if let Some(g) = group {
                task.group = g.map(|s| s.into());
            }
            super::tag_model::apply_tags(&mut task.tags, tags, primary);
            if !tags.is_empty() || primary.is_some() {
                task.handle = None;
            }
            if let Some(handle) = handle {
                task.handle = Some(handle.into());
            }
            task.updated_at = now_ms;
            Ok(())
        })
    }

    pub fn update_task_lane(
        &self,
        plan_id: &str,
        task_id: &str,
        lane: super::TaskLaneUpdate,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            apply_lane_update(&mut task.lane, lane);
            task.lane = normalize_lane_metadata(std::mem::take(&mut task.lane));
            if task.assigned_pane_id.is_none() {
                task.assigned_pane_id = task.lane.pane_id.clone();
            }
            task.updated_at = now_ms;
            Ok(())
        })
    }

    pub fn update_task_quality(
        &self,
        plan_id: &str,
        task_id: &str,
        acceptance_criteria: Option<Vec<String>>,
        test_plan: Option<Option<&str>>,
        correctness: Option<Option<&str>>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            if let Some(criteria) = acceptance_criteria {
                task.acceptance_criteria = criteria;
            }
            if let Some(plan) = test_plan {
                task.test_plan = plan.map(str::to_string);
            }
            if let Some(vector) = correctness {
                task.correctness = vector.map(str::to_string);
            }
            task.updated_at = now_ms;
            Ok(())
        })
    }

    pub fn assign_task(
        &self,
        plan_id: &str,
        task_id: &str,
        pane: &str,
        peer: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            task.assigned_pane_id = Some(pane.into());
            task.lane.pane_id = Some(pane.into());
            task.taken_by_role = None;
            task.peer = peer.map(|s| s.into());
            task.updated_at = now_ms;
            Ok(())
        })
    }

    pub fn rm_task(&self, plan_id: &str, task_id: &str, now_ms: u64) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let pos = p
                .tasks
                .iter()
                .position(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            p.tasks.remove(pos);
            Ok(())
        })
    }

    pub fn update_milestone(
        &self,
        plan_id: &str,
        task_id: &str,
        ms_id: &str,
        kind: Option<&str>,
        text: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let ms = task
                .milestones
                .iter_mut()
                .find(|m| m.id == ms_id)
                .ok_or(PlanError::NotFound)?;
            if let Some(k) = kind {
                let declared = p.milestone_kinds.iter().any(|mk| mk.key == k);
                if !declared {
                    return Err(PlanError::UndeclaredKind {
                        declared: p.milestone_kinds.iter().map(|mk| mk.key.clone()).collect(),
                    });
                }
                ms.kind = k.into();
            }
            if let Some(t) = text {
                ms.text = t.into();
            }
            Ok(())
        })
    }

    pub fn rm_milestone(
        &self,
        plan_id: &str,
        task_id: &str,
        ms_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let pos = task
                .milestones
                .iter()
                .position(|m| m.id == ms_id)
                .ok_or(PlanError::NotFound)?;
            task.milestones.remove(pos);
            Ok(())
        })
    }

    pub fn add_note(
        &self,
        plan_id: &str,
        text: &str,
        links: Vec<String>,
        x: Option<f64>,
        y: Option<f64>,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let eid = gen_id("el");
        let el = CanvasElement {
            id: eid.clone(),
            kind: "note".into(),
            canvas: crate::plan::ElementCanvas {
                x,
                y,
                collapsed: false,
            },
            content: serde_json::json!({ "text": text }),
            links,
        };
        self.with_plan(plan_id, now_ms, |p| {
            p.elements.push(el);
            Ok(())
        })?;
        Ok(eid)
    }

    pub fn add_element(
        &self,
        plan_id: &str,
        kind: &str,
        content: serde_json::Value,
        links: Vec<String>,
        x: Option<f64>,
        y: Option<f64>,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let eid = gen_id("el");
        let el = CanvasElement {
            id: eid.clone(),
            kind: kind.into(),
            canvas: crate::plan::ElementCanvas {
                x,
                y,
                collapsed: false,
            },
            content,
            links,
        };
        self.with_plan(plan_id, now_ms, |p| {
            p.elements.push(el);
            Ok(())
        })?;
        Ok(eid)
    }

    pub fn set_layout(
        &self,
        plan_id: &str,
        element_id: &str,
        x: Option<f64>,
        y: Option<f64>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            // Match the FULL id the frontend sends (node.id). Ids carry their own
            // unique prefix (sec_/task_/ms_/el_), so no namespace parsing is needed —
            // the old split_once(':') rejected the underscore ids and 404'd every drag.
            let found = set_section_canvas(&mut p.sections, element_id, x, y)
                || p.tasks
                    .iter_mut()
                    .find(|t| t.id == element_id)
                    .is_some_and(|t| {
                        t.canvas.x = x;
                        t.canvas.y = y;
                        true
                    })
                || p.tasks
                    .iter_mut()
                    .flat_map(|t| t.milestones.iter_mut())
                    .find(|m| m.id == element_id)
                    .is_some_and(|m| {
                        m.canvas.x = x;
                        m.canvas.y = y;
                        true
                    })
                || p.elements
                    .iter_mut()
                    .find(|e| e.id == element_id)
                    .is_some_and(|e| {
                        e.canvas.x = x;
                        e.canvas.y = y;
                        true
                    });
            if found {
                Ok(())
            } else {
                Err(PlanError::NotFound)
            }
        })
    }
}
