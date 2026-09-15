use super::*;
use crate::service_plan::{retrospective, PlanIncident};

impl PlanStore {
    pub fn ensure_task_creation_allowed(
        &self,
        plan_id: &str,
        _kind: &str,
        _force: bool,
    ) -> Result<(), PlanError> {
        self.get(plan_id).ok_or(PlanError::NotFound).map(|_| ())
    }

    pub fn update_plan(
        &self,
        plan_id: &str,
        title: Option<&str>,
        status: Option<PlanStatus>,
        mode: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let status_for_write = status.clone();
        let write = self.with_plan(plan_id, now_ms, |p| {
            if let Some(t) = title {
                p.title = t.into();
            }
            if let Some(s) = status_for_write {
                if s == PlanStatus::Active && p.status == PlanStatus::Archived {
                    return Err(PlanError::PlanActivationRequiresUnarchive {
                        plan_id: plan_id.into(),
                    });
                }
                p.status = s;
            }
            if let Some(mode) = mode {
                p.mode = parse_plan_mode(mode)?;
            }
            Ok(())
        });
        pending_after_verified(
            write,
            self.verify_plan_updated(plan_id, title, status, mode),
            Some(plan_id.to_string()),
        )
    }

    pub fn set_planner_ref(
        &self,
        plan_id: &str,
        planner_ref: Option<PlannerRef>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let expected = planner_ref.clone();
        let write = self.with_plan(plan_id, now_ms, |p| {
            p.planner = planner_ref;
            Ok(())
        });
        pending_after_verified(
            write,
            self.verify_planner_ref(plan_id, &expected),
            Some(plan_id.to_string()),
        )
    }

    pub fn resolve_planner_tab<F>(
        &self,
        plan_id: &str,
        tab_probe: F,
    ) -> crate::plan::peer::PeerResolution
    where
        F: Fn(&str) -> crate::plan::reconcile::TabProbe,
    {
        let Some(plan) = self.get(plan_id) else {
            return crate::plan::peer::PeerResolution::Unbound;
        };
        crate::plan::peer::resolve_planner_ref(&plan.planner, &plan.owner, tab_probe)
    }

    pub fn set_mode(&self, plan_id: &str, mode: PlanMode, now_ms: u64) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |p| {
            if mode == PlanMode::Paused && p.mode != PlanMode::Paused {
                p.paused_from_mode = Some(p.mode.clone());
            } else if mode != PlanMode::Paused {
                p.paused_from_mode = None;
            }
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

    pub(crate) fn with_plan<T, F>(&self, id: &str, now_ms: u64, f: F) -> Result<T, PlanError>
    where
        F: FnOnce(&mut Plan) -> Result<T, PlanError>,
    {
        let _writer_turn = self.mutation_writer.enter();
        self.with_plan_in_writer(id, now_ms, f)
    }

    pub(super) fn with_plan_in_writer<T, F>(
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
        let mut staged = arc.lock().clone();
        let previous = staged.clone();
        let out = f(&mut staged)?;
        let existing_ids: std::collections::HashSet<_> =
            previous.tasks.iter().map(|task| task.id.as_str()).collect();
        for task in &staged.tasks {
            if !existing_ids.contains(task.id.as_str()) {
                codeterm_types::plan::work_facet::validate_work_facet(
                    staged.tag_facets.iter().map(|facet| facet.key.as_str()),
                    task.tags.iter().map(String::as_str),
                )
                .map_err(PlanError::WorkFacetViolation)?;
            }
        }
        staged.updated_at = now_ms;
        if staged.canonical_project_path.trim().is_empty() {
            staged.canonical_project_path = staged.project_path.clone();
        }
        super::super::store_index::record_lifecycle_events(&mut staged, Some(&previous), now_ms);
        super::tag_model::normalize_plan(&mut staged);
        let path = self
            .persist_plan_and_spec(&staged)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        let committed = {
            let mut guard = arc.lock();
            *guard = staged;
            guard.clone()
        };
        self.note_plan_write(&committed, &path);
        self.record_committed_snapshot(&committed);
        let entry = Self::index_entry(&committed, path);
        self.index.write().insert(committed.id.clone(), entry);
        self.reset_bound_tabs(&committed);
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
        self.add_task_with_external_refs(plan_id, new, &[], now_ms)
    }

    pub fn add_task_with_external_refs(
        &self,
        plan_id: &str,
        new: NewTask,
        external_refs: &[String],
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let external_refs =
            crate::service_plan::external_refs::canonicalize_external_refs(external_refs)?;
        let tid = self.mint_id("task");
        let assigned_tab_id = new
            .assigned_tab_id
            .clone()
            .or_else(|| new.lane.tab_id.clone());
        let mut lane = normalize_lane_metadata(new.lane);
        if lane.tab_id.is_none() {
            lane.tab_id = assigned_tab_id.clone();
        }
        let key = self.issue_task_keys(plan_id, 1).into_iter().next();
        let mut task = Task {
            id: tid.clone(),
            name: new.name,
            kind: new.kind,
            model: new.model,
            model_rationale: new.model_rationale,
            acceptance_criteria: new.acceptance_criteria,
            external_refs,
            test_plan: new.test_plan,
            qa_plan: new.qa_plan,
            correctness: new.correctness,
            status: TaskStatus::Pending,
            review_stage: Default::default(),
            qa_state: None,
            review_round: 0,
            review_oracle: None,
            review_evidence: vec![],
            key,
            review_override: None,
            review_audit: vec![],
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
            personality: new.personality,
            personality_provenance: new.personality_provenance,
            competition: new.competition,
            intent: new.intent,
            execution_intent: new.execution_intent,
            parent_task_id: new.parent_task_id,
            depends_on: normalized_dependency_ids(&new.depends_on),
            assigned_tab_id,
            lane,
            taken_by_role: None,
            peer: None,
            group: new.group,
            canvas: Default::default(),
            milestones: vec![],
            execution_attempts: vec![],
            pause_reason: None,
            interruption: Default::default(),
            attachments: vec![],
        };
        let verify_sections = !task.section_ids.is_empty();
        let mut expected = Self::task_add_expectation(&task, verify_sections);
        let write = self.with_plan(plan_id, now_ms, |p| {
            let resolved_depends_on = validate_task_dependencies(p, &tid, &task.depends_on)?;
            task.depends_on = resolved_depends_on.clone();
            if task.section_ids.is_empty() {
                task.section_ids = super::section_links::auto_section_ids_for_task(p, &task);
            }
            p.tasks.push(task);
            Ok(resolved_depends_on)
        });
        // depends_on refs (e.g. handles) resolve to canonical task ids during the
        // write; the expectation must track the resolved form or readback always fails.
        let write = match write {
            Ok(resolved_depends_on) => {
                expected.depends_on = resolved_depends_on;
                Ok(())
            }
            Err(err) => Err(err),
        };
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
            let task_index = crate::plan::task_key::find_task_index(&p.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let was_in_review = p.tasks[task_index].status == TaskStatus::InReview;
            super::finalize_task_done(p, task_index, now_ms)?;
            if was_in_review {
                p.tasks[task_index].review_evidence.push(ReviewEvidence {
                    id: crate::plan::ids::short_id("rev"),
                    from: from.clone(),
                    verdict: ReviewVerdict::Accept,
                    note: result.unwrap_or("").into(),
                    at: now_ms,
                    changed_executable_surfaces: false,
                    distinct_oracle: false,
                });
                crate::service_plan::review_gate::earn_review_credit(&mut p.review_policy);
            }
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

    pub fn close_task_authorized(
        &self,
        plan_id: &str,
        task_id: &str,
        result: Option<&str>,
        from: AgentRef,
        authority: &crate::service_plan::interruption::TaskMutationAuthority,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let task_index = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            crate::service_plan::interruption::authorize_task_mutation(
                plan,
                &plan.tasks[task_index],
                authority,
            )?;
            let was_in_review = plan.tasks[task_index].status == TaskStatus::InReview;
            super::finalize_task_done(plan, task_index, now_ms)?;
            if was_in_review {
                plan.tasks[task_index].review_evidence.push(ReviewEvidence {
                    id: crate::plan::ids::short_id("rev"),
                    from: from.clone(),
                    verdict: ReviewVerdict::Accept,
                    note: result.unwrap_or("").into(),
                    at: now_ms,
                    changed_executable_surfaces: false,
                    distinct_oracle: false,
                });
                crate::service_plan::review_gate::earn_review_credit(&mut plan.review_policy);
            }
            if let Some(text) = result {
                let milestone_id =
                    crate::plan::ids::mint_id("ms", |candidate| plan_contains_id(plan, candidate));
                plan.tasks[task_index].milestones.push(Milestone {
                    id: milestone_id,
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
        self.archive_with_incident(plan_id, None, now_ms)
    }

    /// Archive status and its incident evidence are ONE write. `with_plan` commits
    /// only after the file persists, so neither lands without the other.
    pub fn archive_with_incident(
        &self,
        plan_id: &str,
        incident: Option<PlanIncident>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let mid = self.mint_id("ms");
        self.with_plan(plan_id, now_ms, |p| {
            if let Some(incident) = incident.as_ref() {
                if incident.kind.trim().is_empty() || incident.text.trim().is_empty() {
                    return Err(PlanError::Io(
                        "plan archive incident requires a kind and text".into(),
                    ));
                }
                p.strides.push(Milestone {
                    id: mid.clone(),
                    from: incident.from.clone(),
                    kind: incident.kind.clone(),
                    text: incident.text.clone(),
                    ts: now_ms,
                    links: vec![],
                    loop_id: None,
                    iteration: None,
                    canvas: Default::default(),
                    attachments: vec![],
                });
            }
            p.status = PlanStatus::Archived;
            Ok(())
        })
    }

    pub fn unarchive(&self, plan_id: &str, now_ms: u64) -> Result<(), PlanError> {
        let _writer_turn = self.mutation_writer.enter();
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let team_id = plan
            .team_id
            .as_deref()
            .filter(|team_id| !team_id.trim().is_empty());
        #[cfg(not(test))]
        let team_id = Some(team_id.ok_or_else(|| PlanError::ActivePlanRequiresTeam {
            plan_id: plan_id.into(),
        })?);
        if let Some(team_id) = team_id {
            if let Some((existing_plan_id, existing_title)) = self.active_plan_for_team(team_id) {
                if existing_plan_id != plan_id {
                    return Err(PlanError::TeamAlreadyHasActivePlan {
                        team_id: team_id.into(),
                        existing_plan_id,
                        existing_title,
                    });
                }
            }
        }
        self.with_plan_in_writer(plan_id, now_ms, |p| {
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

    /// Refresh the current owner's live session after its tab restarted.
    /// A restart is not a handover, so no `owner_history` entry is recorded.
    pub fn refresh_owner_session(
        &self,
        plan_id: &str,
        tab_id: &str,
        session_id: Option<&str>,
        title: Option<&str>,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        let mut refreshed = false;
        self.with_plan(plan_id, now_ms, |p| {
            if p.owner.tab_id.as_deref() != Some(tab_id) {
                return Ok(());
            }
            let session_id = session_id.map(str::to_string);
            let title = title.map(str::to_string).or_else(|| p.owner.title.clone());
            if p.owner.session_id != session_id || p.owner.title != title {
                p.owner.session_id = session_id;
                p.owner.title = title;
                refreshed = true;
            }
            Ok(())
        })?;
        Ok(refreshed)
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
    use crate::service_plan::{NewTask, PlanError, PlanStore};
    use tempfile::TempDir;

    fn store() -> (TempDir, PlanStore, String) {
        let dir = TempDir::new().unwrap();
        let s = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Gate".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
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
    fn drafting_records_execution_tasks_without_unlocking_them() {
        let (_d, s, pid) = store();
        assert!(add(&s, &pid, "implement").is_ok());
        assert!(add(&s, &pid, "planning").is_ok());
        assert!(!s.get(&pid).unwrap().execution_tasks_unlocked());
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
                tab_id: "tab_p".into(),
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
                tab_id: "tab_p".into()
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

#[cfg(test)]
mod team_binding_tests {
    use std::sync::{Arc, Barrier};

    use crate::plan::{OrchestratorRef, PlanMode, PlanStatus};
    use crate::service_plan::{retrospective, PlanError, PlanStore};
    use tempfile::TempDir;

    fn store() -> (TempDir, PlanStore) {
        let dir = TempDir::new().unwrap();
        let s = PlanStore::new_for_test(dir.path());
        (dir, s)
    }

    fn create(
        s: &PlanStore,
        dir: &TempDir,
        title: &str,
        team: &str,
        now: u64,
    ) -> Result<crate::plan::Plan, PlanError> {
        s.create_plan_for_team(
            title.into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            team.to_string(),
            vec![],
            now,
        )
    }

    #[test]
    fn teamless_creator_is_rejected_without_a_fallback_plan() {
        let (dir, s) = store();
        let error = create(&s, &dir, "A", "", 1).unwrap_err();
        assert!(matches!(error, PlanError::ActivePlanRequiresTeam { .. }));
        assert!(s.list_index().is_empty());
    }

    #[test]
    fn team_bound_creation_persists_team_id() {
        let (dir, s) = store();
        let plan = create(&s, &dir, "Bound", "team_alpha", 1).unwrap();
        assert_eq!(plan.team_id.as_deref(), Some("team_alpha"));
        assert_eq!(
            s.active_plan_for_team("team_alpha"),
            Some((plan.id.clone(), "Bound".into()))
        );
        // Exposed in the plan index.
        let entry = s
            .list_index()
            .into_iter()
            .find(|e| e.plan_id == plan.id)
            .unwrap();
        assert_eq!(entry.team_id.as_deref(), Some("team_alpha"));
    }

    #[test]
    fn team_close_archives_plan_until_atomic_adoption() {
        let (dir, s) = store();
        let plan = create(&s, &dir, "Bound", "team_closed", 1).unwrap();

        assert_eq!(
            s.archive_and_detach_team_plans("team_closed", 2).unwrap(),
            vec![plan.id.clone()]
        );
        let archived = s.get(&plan.id).unwrap();
        assert_eq!(archived.team_id, None);
        assert_eq!(archived.status, PlanStatus::Archived);

        assert!(matches!(
            s.resume_plan(&plan.id, 3),
            Err(PlanError::ActivePlanRequiresTeam { .. })
        ));
        let rebound = s
            .adopt_plan_for_team(
                &plan.id,
                "team_new",
                OrchestratorRef::from_tab("new-owner"),
                4,
            )
            .unwrap();
        assert_eq!(rebound.team_id.as_deref(), Some("team_new"));
        assert_eq!(rebound.status, PlanStatus::Active);
        assert_eq!(rebound.owner.tab_id.as_deref(), Some("new-owner"));
    }

    #[test]
    fn dangling_team_reconciliation_archives_every_active_orphan() {
        let (dir, s) = store();
        let parked = create(&s, &dir, "Parked", "team_missing", 1).unwrap();
        s.set_mode(&parked.id, PlanMode::Paused, 2).unwrap();
        let done = create(&s, &dir, "Done", "team_done", 3).unwrap();
        s.set_mode(&done.id, PlanMode::Complete, 4).unwrap();
        let archived = create(&s, &dir, "Archived", "team_archived", 5).unwrap();
        s.update_plan(&archived.id, None, Some(PlanStatus::Archived), None, 6)
            .unwrap();
        let healthy = create(&s, &dir, "Healthy", "team_live", 7).unwrap();
        let live = std::collections::BTreeSet::from(["team_live".to_string()]);

        let mut expected = vec![done.id.clone(), parked.id.clone()];
        expected.sort();
        assert_eq!(
            s.archive_active_plans_without_live_team(&live, 8).unwrap(),
            expected
        );
        assert_eq!(s.get(&parked.id).unwrap().status, PlanStatus::Archived);
        assert_eq!(s.get(&done.id).unwrap().status, PlanStatus::Archived);
        assert_eq!(s.get(&archived.id).unwrap().status, PlanStatus::Archived);
        assert_eq!(
            s.get(&healthy.id).unwrap().team_id.as_deref(),
            Some("team_live")
        );
        assert!(s
            .archive_active_plans_without_live_team(&live, 9)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn second_active_plan_hard_refused() {
        let (dir, s) = store();
        let first = create(&s, &dir, "First", "team_alpha", 1).unwrap();
        let before = s.list_index().len();
        let err = create(&s, &dir, "Second", "team_alpha", 2).unwrap_err();
        match &err {
            PlanError::TeamAlreadyHasActivePlan {
                team_id,
                existing_plan_id,
                existing_title,
            } => {
                assert_eq!(team_id, "team_alpha");
                assert_eq!(existing_plan_id, &first.id);
                assert_eq!(existing_title, "First");
            }
            other => panic!("expected hard refuse, got {other:?}"),
        }
        let msg = err.to_string();
        assert!(msg.contains(&first.id), "error must name the existing plan");
        assert!(msg.contains("team_alpha"), "error must name the team");
        assert!(
            msg.to_lowercase().contains("another team"),
            "error must instruct to use another Team: {msg}"
        );
        // Hard refuse leaves the store unchanged — no teamless fallback plan.
        assert_eq!(s.list_index().len(), before);
    }

    #[test]
    fn concurrent_team_plan_creation_allows_exactly_one() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(PlanStore::new_for_test(dir.path()));
        let barrier = Arc::new(Barrier::new(3));
        let root = dir.path().display().to_string();
        let spawn = |title: &'static str, now| {
            let store = store.clone();
            let barrier = barrier.clone();
            let root = root.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.create_plan_for_team(
                    title.into(),
                    root,
                    OrchestratorRef::from_tab(title),
                    "team_alpha".into(),
                    vec![],
                    now,
                )
            })
        };
        let first = spawn("First", 1);
        let second = spawn("Second", 2);
        barrier.wait();
        let results = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(PlanError::TeamAlreadyHasActivePlan { .. })))
                .count(),
            1
        );
        assert_eq!(store.list_index().len(), 1);
    }

    #[test]
    fn concurrent_team_plan_create_and_bind_allow_exactly_one() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(PlanStore::new_for_test(dir.path()));
        let mut teamless = crate::plan::Plan::new(
            "Existing".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        store.save(&mut teamless, 1).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let bind_store = store.clone();
        let bind_barrier = barrier.clone();
        let bind_plan_id = teamless.id.clone();
        let bind = std::thread::spawn(move || {
            bind_barrier.wait();
            bind_store.bind_plan_to_team(&bind_plan_id, "team_alpha", 2)
        });
        let create_store = store.clone();
        let create_barrier = barrier.clone();
        let root = dir.path().display().to_string();
        let create = std::thread::spawn(move || {
            create_barrier.wait();
            create_store.create_plan_for_team(
                "New".into(),
                root,
                OrchestratorRef::from_tab("orch"),
                "team_alpha".into(),
                vec![],
                3,
            )
        });
        barrier.wait();
        let results = [bind.join().unwrap(), create.join().unwrap()];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(PlanError::TeamAlreadyHasActivePlan { .. })))
                .count(),
            1
        );
        assert!(store.active_plan_for_team("team_alpha").is_some());
    }

    #[test]
    fn different_team_gets_its_own_plan() {
        let (dir, s) = store();
        create(&s, &dir, "Alpha", "team_alpha", 1).unwrap();
        let beta = create(&s, &dir, "Beta", "team_beta", 2).unwrap();
        assert_eq!(beta.team_id.as_deref(), Some("team_beta"));
    }

    /// Advisory, not a gate: a missing retrospective must never trap closure.
    #[test]
    fn archive_succeeds_without_retrospective() {
        let (dir, s) = store();
        let plan = create(&s, &dir, "Retro advisory", "team_alpha", 1).unwrap();
        assert!(!retrospective::has_retrospective(&s.get(&plan.id).unwrap()));

        s.archive(&plan.id, 2).unwrap();

        assert_eq!(s.get(&plan.id).unwrap().status, PlanStatus::Archived);
    }

    #[test]
    fn archiving_frees_the_team_for_a_new_plan() {
        let (dir, s) = store();
        let first = create(&s, &dir, "First", "team_alpha", 1).unwrap();
        assert!(matches!(
            create(&s, &dir, "Second", "team_alpha", 2),
            Err(PlanError::TeamAlreadyHasActivePlan { .. })
        ));
        s.archive(&first.id, 3).unwrap();
        // The 1:1 rule counts only ACTIVE plans — an archived one no longer blocks.
        let second = create(&s, &dir, "Second", "team_alpha", 4).unwrap();
        assert_eq!(second.team_id.as_deref(), Some("team_alpha"));
        assert_ne!(first.id, second.id);
    }
}

#[cfg(test)]
mod writer_queue_tests;
