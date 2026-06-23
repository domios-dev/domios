use super::{NewTask, PlanError, PlanStore};
use crate::plan::loop_engine::stop_for;
use crate::plan::{
    AgentRef, CanvasElement, LoopSpec, LoopStage, LoopStatus, Milestone, MilestoneKind,
    OrchestratorRef, Plan, PlanStatus, Section, Task, TaskStatus, TransferRecord,
};

fn gen_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

fn push_subsection(
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

fn update_section_fields(
    sections: &mut [Section],
    id: &str,
    title: Option<&str>,
    body: Option<&str>,
) -> bool {
    for s in sections.iter_mut() {
        if s.id == id {
            if let Some(t) = title {
                s.title = t.into();
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

fn rm_section_tree(sections: &mut Vec<Section>, id: &str) -> bool {
    if let Some(pos) = sections.iter().position(|s| s.id == id) {
        sections.remove(pos);
        return true;
    }
    sections
        .iter_mut()
        .any(|s| rm_section_tree(&mut s.subsections, id))
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

impl PlanStore {
    pub fn create_plan(
        &self,
        title: String,
        project_path: String,
        owner: OrchestratorRef,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
    ) -> Result<Plan, PlanError> {
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

    fn with_plan<T, F>(&self, id: &str, now_ms: u64, f: F) -> Result<T, PlanError>
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
        let path = Self::write_atomic(&guard).map_err(|e| PlanError::Io(e.to_string()))?;
        self.note_write(&path);
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
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let sid = gen_id("sec");
        let section = Section {
            id: sid.clone(),
            title: title.into(),
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
        let task = Task {
            id: tid.clone(),
            name: new.name,
            kind: new.kind,
            model: new.model,
            model_rationale: new.model_rationale,
            status: TaskStatus::Pending,
            status_label: None,
            section_ids: new.section_ids,
            body: new.body,
            intent: new.intent,
            assigned_pane_id: new.assigned_pane_id,
            peer: None,
            canvas: Default::default(),
            milestones: vec![],
        };
        self.with_plan(plan_id, now_ms, |p| {
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
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or(PlanError::TaskNotFound)?;
            task.status = TaskStatus::Done;
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
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            if update_section_fields(&mut p.sections, section_id, title, body) {
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
        body: Option<&str>,
        intent: Option<Option<&str>>,
        section_ids: Option<Vec<String>>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or(PlanError::TaskNotFound)?;
            if let Some(n) = name {
                task.name = n.into();
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
            if let Some(s) = status {
                task.status = parse_task_status(s)?;
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
                .find(|t| t.id == task_id)
                .ok_or(PlanError::TaskNotFound)?;
            task.assigned_pane_id = Some(pane.into());
            task.peer = peer.map(|s| s.into());
            Ok(())
        })
    }

    pub fn rm_task(&self, plan_id: &str, task_id: &str, now_ms: u64) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let pos = p
                .tasks
                .iter()
                .position(|t| t.id == task_id)
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

    pub fn create_loop(
        &self,
        plan_id: &str,
        target: &str,
        stages: Vec<LoopStage>,
        budget: u32,
        stop_when: Option<&str>,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let lid = gen_id("loop");
        let spec = LoopSpec {
            id: lid.clone(),
            target: target.into(),
            stages,
            budget,
            count: 0,
            stop_when: stop_when.map(|s| s.into()),
            status: LoopStatus::Running,
        };
        self.with_plan(plan_id, now_ms, |p| {
            p.loops.push(spec);
            Ok(())
        })?;
        Ok(lid)
    }

    pub fn set_loop(
        &self,
        loop_id: &str,
        budget: Option<u32>,
        stop_when: Option<Option<&str>>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let plan_id = self
            .find_plan_with_loop(loop_id)
            .ok_or(PlanError::NotFound)?;
        self.with_plan(&plan_id, now_ms, |p| {
            let lp = p
                .loops
                .iter_mut()
                .find(|l| l.id == loop_id)
                .ok_or(PlanError::NotFound)?;
            if let Some(b) = budget {
                lp.budget = b;
            }
            if let Some(sw) = stop_when {
                lp.stop_when = sw.map(|s| s.into());
            }
            Ok(())
        })
    }

    pub fn loop_status(&self, loop_id: &str) -> Option<LoopSpec> {
        let plan_id = self.find_plan_with_loop(loop_id)?;
        self.get(&plan_id)
            .and_then(|p| p.loops.iter().find(|l| l.id == loop_id).cloned())
    }

    pub fn stop_loop(&self, loop_id: &str, now_ms: u64) -> Result<(), PlanError> {
        let plan_id = self
            .find_plan_with_loop(loop_id)
            .ok_or(PlanError::NotFound)?;
        self.with_plan(&plan_id, now_ms, |p| {
            let lp = p
                .loops
                .iter_mut()
                .find(|l| l.id == loop_id)
                .ok_or(PlanError::NotFound)?;
            stop_for(lp);
            Ok(())
        })
    }

    fn find_plan_with_loop(&self, loop_id: &str) -> Option<String> {
        for entry in self.list_index() {
            if let Some(plan) = self.get(&entry.plan_id) {
                if plan.loops.iter().any(|l| l.id == loop_id) {
                    return Some(entry.plan_id);
                }
            }
        }
        None
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
            let (prefix, raw) = element_id.split_once(':').ok_or(PlanError::NotFound)?;
            let found = match prefix {
                "section" => set_section_canvas(&mut p.sections, raw, x, y),
                "task" => p.tasks.iter_mut().find(|t| t.id == raw).is_some_and(|t| {
                    t.canvas.x = x;
                    t.canvas.y = y;
                    true
                }),
                "ms" => p
                    .tasks
                    .iter_mut()
                    .flat_map(|t| t.milestones.iter_mut())
                    .find(|m| m.id == raw)
                    .is_some_and(|m| {
                        m.canvas.x = x;
                        m.canvas.y = y;
                        true
                    }),
                "el" => p
                    .elements
                    .iter_mut()
                    .find(|e| e.id == raw)
                    .is_some_and(|e| {
                        e.canvas.x = x;
                        e.canvas.y = y;
                        true
                    }),
                _ => false,
            };
            if found {
                Ok(())
            } else {
                Err(PlanError::NotFound)
            }
        })
    }
}
