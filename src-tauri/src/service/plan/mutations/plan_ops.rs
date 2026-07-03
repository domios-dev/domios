use super::*;
use crate::service::plan::retrospective;

impl PlanStore {
    pub fn ensure_task_creation_allowed(
        &self,
        plan_id: &str,
        kind: &str,
        force: bool,
    ) -> Result<(), PlanError> {
        if force {
            return Ok(());
        }
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        // Planning-kind tasks are the planning workflow itself (improve GOAL/spec/plan/task-graph)
        // and delegated planning subwork — always allowed regardless of state.
        if kind == "planning" {
            return Ok(());
        }
        // Single gate: execution-task creation unlocks only once the plan state
        // says so (planner-advanced planning_state, or an already-approved lifecycle).
        if plan.execution_tasks_unlocked() {
            Ok(())
        } else {
            Err(PlanError::TaskCreationRequiresApproval)
        }
    }

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
        if self.id_taken(&plan.id) {
            plan.id = self.mint_id("pl");
        }
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
        mode: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            if let Some(t) = title {
                p.title = t.into();
            }
            if let Some(s) = status {
                p.status = s;
            }
            if let Some(mode) = mode {
                p.mode = parse_plan_mode(mode)?;
            }
            Ok(())
        })
    }

    pub fn set_planner_ref(
        &self,
        plan_id: &str,
        planner_ref: Option<PlannerRef>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            p.planner = planner_ref;
            Ok(())
        })
    }

    pub fn resolve_planner_pane<F>(
        &self,
        plan_id: &str,
        pane_probe: F,
    ) -> crate::plan::peer::PeerResolution
    where
        F: Fn(&str) -> crate::plan::reconcile::PaneProbe,
    {
        let Some(plan) = self.get(plan_id) else {
            return crate::plan::peer::PeerResolution::Unbound;
        };
        crate::plan::peer::resolve_planner_ref(&plan.planner, &plan.owner, pane_probe)
    }

    pub fn set_mode(&self, plan_id: &str, mode: PlanMode, now_ms: u64) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            p.mode = mode;
            Ok(())
        })
    }

    /// Advance the planner-owned planning_state, recording author + working point.
    /// Mirrors the lifecycle `mode` so heartbeat/next-actions stay consistent while
    /// the single execution-task gate keys off planning_state. Idempotent no-op if
    /// already in the target state.
    pub fn set_planning_state(
        &self,
        plan_id: &str,
        to: PlanningState,
        by: PlanningAuthor,
        note: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            let from = p.planning_state;
            if from == to {
                return Ok(());
            }
            p.planning_log.push(PlanningTransition {
                from,
                to,
                by,
                note: note
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(str::to_string),
                at: now_ms,
            });
            p.planning_state = to;
            // Keep lifecycle mode in lockstep with the planning<->approved boundary;
            // never override an execution/complete lifecycle.
            match (to.allows_execution_tasks(), &p.mode) {
                (true, PlanMode::Planning) => p.mode = PlanMode::Approved,
                (false, PlanMode::Approved) => p.mode = PlanMode::Planning,
                _ => {}
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

    pub(in crate::service::plan) fn with_plan<T, F>(
        &self,
        id: &str,
        now_ms: u64,
        f: F,
    ) -> Result<T, PlanError>
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
        if guard.canonical_project_path.trim().is_empty() {
            guard.canonical_project_path = guard.project_path.clone();
        }
        super::tag_model::normalize_plan(&mut guard);
        let path = Self::write_atomic(&guard).map_err(|e| PlanError::Io(e.to_string()))?;
        self.note_plan_write(&guard, &path);
        self.mirror_plan_index(&guard)
            .map_err(super::plan_index_write_error)?;
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
        let sid = self.mint_id("sec");
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
            attachments: vec![],
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
        let tid = self.mint_id("task");
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
            review_stage: Default::default(),
            qa_state: None,
            status_label: None,
            priority: new.priority.unwrap_or_default(),
            created_at: now_ms,
            updated_at: now_ms,
            status_changed_at: now_ms,
            handle: new.handle,
            tags: new.tags,
            section_ids: new.section_ids,
            body: new.body,
            role_guidance: new.role_guidance,
            executor_role: new.executor_role,
            competition: new.competition,
            intent: new.intent,
            parent_task_id: new.parent_task_id,
            assigned_pane_id,
            lane,
            taken_by_role: None,
            peer: None,
            group: new.group,
            canvas: Default::default(),
            milestones: vec![],
            execution_attempts: vec![],
            park_reason: None,
            attachments: vec![],
        };
        let verify_sections = !task.section_ids.is_empty();
        let expected = Self::task_add_expectation(&task, verify_sections);
        let write = self.with_plan(plan_id, now_ms, |p| {
            if task.section_ids.is_empty() {
                task.section_ids = super::section_links::auto_section_ids_for_task(p, &task);
            }
            p.tasks.push(task);
            Ok(())
        });
        pending_after_verified(
            write,
            self.verify_task_added(plan_id, &tid, &expected),
            Some(tid.clone()),
        )?;
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
        let write = self.with_plan(plan_id, now_ms, |p| {
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
            super::qa::normalize_task_review_fields(task);
            let assigned = task.assigned_pane_id.clone();
            super::task_attempts::mark_task_assignment_closed(task, assigned.as_deref(), now_ms);
            clear_task_assignment(task);
            task.updated_at = now_ms;
            if let Some(text) = result {
                let mid = crate::plan::ids::mint_id("ms", |c| plan_contains_id(p, c));
                p.tasks[task_index].milestones.push(Milestone {
                    id: mid,
                    from,
                    kind: "status_update".into(),
                    text: text.into(),
                    ts: now_ms,
                    links: vec![],
                    loop_id: None,
                    iteration: None,
                    canvas: Default::default(),
                    attachments: vec![],
                });
            }
            Ok(())
        });
        pending_after_verified(
            write,
            self.verify_task_closed(plan_id, task_id, result),
            Some(task_id.to_string()),
        )
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
        let mid = self.mint_id("ms");
        let write = self.with_plan(plan_id, now_ms, |p| {
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
                attachments: vec![],
            });
            Ok(())
        });
        pending_after_verified(
            write,
            self.verify_task_milestone_added(plan_id, task_id, &mid, kind, text),
            Some(task_id.to_string()),
        )?;
        Ok(mid)
    }

    // Append a plan-level, task-less working point (surfaced in recent_strides).
    pub fn add_plan_stride(
        &self,
        plan_id: &str,
        kind: &str,
        text: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let mid = self.mint_id("ms");
        self.with_plan(plan_id, now_ms, |p| {
            p.strides.push(Milestone {
                id: mid.clone(),
                from,
                kind: kind.into(),
                text: text.into(),
                ts: now_ms,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            Ok(())
        })?;
        Ok(mid)
    }

    pub fn add_retrospective(
        &self,
        plan_id: &str,
        body: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let text = body.trim();
        if text.is_empty() {
            return Err(PlanError::GateRequiresQuestion);
        }
        let mid = self.mint_id("ms");
        let supersede_id = self.mint_id("ms");
        self.with_plan(plan_id, now_ms, |p| {
            if retrospective::latest_retrospective(p).is_some() {
                p.strides.push(Milestone {
                    id: supersede_id.clone(),
                    from: from.clone(),
                    kind: retrospective::RETROSPECTIVE_KIND.into(),
                    text: retrospective::retrospective_supersede_note().into(),
                    ts: now_ms,
                    links: vec![],
                    loop_id: None,
                    iteration: None,
                    canvas: Default::default(),
                    attachments: vec![],
                });
            }
            p.strides.push(Milestone {
                id: mid.clone(),
                from,
                kind: retrospective::RETROSPECTIVE_KIND.into(),
                text: text.into(),
                ts: now_ms,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
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
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let mut removed = vec![];
        collect_section_tree_ids(&plan.sections, section_id, &mut removed);
        self.with_plan(plan_id, now_ms, |p| {
            if rm_section_tree(&mut p.sections, section_id) {
                Ok(())
            } else {
                Err(PlanError::NotFound)
            }
        })?;
        for section_id in removed {
            self.remove_section_attachment_dir(&plan, &section_id)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod planning_state_tests {
    use crate::plan::{OrchestratorRef, Plan, PlanMode, PlanningAuthor, PlanningState};
    use crate::service::plan::{NewTask, PlanError, PlanStore};
    use tempfile::TempDir;

    fn store() -> (TempDir, PlanStore, String) {
        let dir = TempDir::new().unwrap();
        let s = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Gate".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane("orch"),
        );
        s.save(&mut plan, 1).unwrap();
        (dir, s, plan.id)
    }

    fn add(store: &PlanStore, pid: &str, kind: &str) -> Result<String, PlanError> {
        store.ensure_task_creation_allowed(pid, kind, false)?;
        store.add_task(
            pid,
            NewTask {
                name: "t".into(),
                kind: kind.into(),
                ..Default::default()
            },
            2,
        )
    }

    #[test]
    fn drafting_blocks_execution_tasks_but_allows_planning_kind() {
        let (_d, s, pid) = store();
        assert!(matches!(
            add(&s, &pid, "implement"),
            Err(PlanError::TaskCreationRequiresApproval)
        ));
        assert!(add(&s, &pid, "planning").is_ok());
    }

    #[test]
    fn approved_state_unlocks_execution_tasks_and_mirrors_mode() {
        let (_d, s, pid) = store();
        s.set_planning_state(&pid, PlanningState::Approved, PlanningAuthor::User, None, 3)
            .unwrap();
        let plan = s.get(&pid).unwrap();
        assert_eq!(plan.planning_state, PlanningState::Approved);
        assert_eq!(plan.mode, PlanMode::Approved);
        assert_eq!(plan.planning_log.len(), 1);
        assert!(add(&s, &pid, "implement").is_ok());
    }

    #[test]
    fn advance_records_author_and_is_idempotent() {
        let (_d, s, pid) = store();
        s.set_planning_state(
            &pid,
            PlanningState::StructureReady,
            PlanningAuthor::Planner {
                pane_id: "pane_p".into(),
            },
            Some("skeleton done"),
            3,
        )
        .unwrap();
        // Same state again — no new log entry.
        s.set_planning_state(
            &pid,
            PlanningState::StructureReady,
            PlanningAuthor::User,
            None,
            4,
        )
        .unwrap();
        let plan = s.get(&pid).unwrap();
        assert_eq!(plan.planning_log.len(), 1);
        assert_eq!(
            plan.planning_log[0].by,
            PlanningAuthor::Planner {
                pane_id: "pane_p".into()
            }
        );
        // StructureReady still locks execution tasks.
        assert!(!plan.execution_tasks_unlocked());
    }

    #[test]
    fn unapprove_returns_to_structure_ready_and_relocks() {
        let (_d, s, pid) = store();
        s.set_planning_state(&pid, PlanningState::Approved, PlanningAuthor::User, None, 3)
            .unwrap();
        s.set_planning_state(
            &pid,
            PlanningState::StructureReady,
            PlanningAuthor::User,
            None,
            4,
        )
        .unwrap();
        let plan = s.get(&pid).unwrap();
        assert_eq!(plan.mode, PlanMode::Planning);
        assert!(!plan.execution_tasks_unlocked());
    }

    #[test]
    fn legacy_approved_mode_bridges_gate_without_planning_state() {
        let (_d, s, pid) = store();
        // Simulate a pre-planning_state plan: mode Approved, planning_state Drafting.
        s.set_mode(&pid, PlanMode::Approved, 3).unwrap();
        let plan = s.get(&pid).unwrap();
        assert_eq!(plan.planning_state, PlanningState::Drafting);
        assert!(plan.execution_tasks_unlocked());
        assert!(add(&s, &pid, "implement").is_ok());
    }
}
