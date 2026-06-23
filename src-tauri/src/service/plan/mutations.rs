use super::{NewTask, PlanError, PlanStore};
use crate::plan::{
    AgentRef, Milestone, MilestoneKind, OrchestratorRef, Plan, PlanStatus, Section, Task,
    TaskStatus, TransferRecord,
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

impl PlanStore {
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
