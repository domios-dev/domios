use super::*;
use crate::service_plan::{retrospective, PlanIncident};

impl PlanStore {
    pub fn ensure_task_creation_allowed(
        &self,
        plan_id: &str,
        _kind: &str,
        _force: bool,
    ) -> Result<(), PlanError> {
        self.database.ensure_plan_exists(plan_id)
    }

    pub fn update_plan(
        &self,
        plan_id: &str,
        title: Option<&str>,
        status: Option<PlanStatus>,
        mode: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        if let Some(title) = title {
            crate::service_plan::text_bounds::validate_plan_label("plan title", title)?;
        }
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
    ) -> Result<crate::plan::peer::PeerResolution, PlanError>
    where
        F: Fn(&str) -> crate::plan::reconcile::TabProbe,
    {
        let Some(plan) = self.planner_gate_context(plan_id, None)? else {
            return Ok(crate::plan::peer::PeerResolution::Unbound);
        };
        Ok(crate::plan::peer::resolve_planner_ref(
            &plan.planner,
            &plan.owner,
            tab_probe,
        ))
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

    pub fn owner_of(&self, id: &str) -> Option<OrchestratorRef> {
        self.plan_arc(id).ok().map(|plan| plan.lock().owner.clone())
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
        crate::service_plan::text_bounds::validate_plan_label("section title", title)?;
        crate::service_plan::text_bounds::validate_plan_text("section body", body)?;
        for tag in tags {
            crate::service_plan::text_bounds::validate_plan_label("section tag", tag)?;
        }
        if let Some(primary) = primary {
            crate::service_plan::text_bounds::validate_plan_label("section primary tag", primary)?;
        }
        if let Some(handle) = handle {
            crate::service_plan::text_bounds::validate_plan_label("section handle", handle)?;
        }
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
        let mut new = new;
        crate::service_plan::text_bounds::validate_new_task_text(&new)?;
        new.personality = new
            .personality
            .as_deref()
            .map(crate::service_plan::worker_identity::validate_executor_name)
            .transpose()
            .map_err(PlanError::InvalidExecutorName)?;
        let external_refs =
            crate::service_plan::external_refs::canonicalize_external_refs(external_refs)?;
        crate::service_plan::text_bounds::validate_external_refs(&external_refs)?;
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
            files: new.files.validated().map_err(PlanError::InvalidPlanText)?,
            id: tid.clone(),
            name: new.name,
            kind: new.kind,
            model: new.model,
            speed: None,
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
        crate::service_plan::text_bounds::validate_task_text(&task)?;
        let verify_sections = !task.section_ids.is_empty();
        let mut expected = Self::task_add_expectation(&task, verify_sections);
        let write = self.with_plan(plan_id, now_ms, |p| {
            if let Some(parent_id) = task.parent_task_id.as_deref() {
                let parent = p
                    .tasks
                    .iter()
                    .find(|candidate| candidate.id == parent_id)
                    .ok_or(PlanError::TaskNotFound)?;
                if parent.parent_task_id.is_some() {
                    return Err(PlanError::InvalidSplitParent {
                        reason: super::SplitParentReason::ChildParent,
                    });
                }
                if matches!(&parent.status, TaskStatus::Done | TaskStatus::Cancelled) {
                    return Err(PlanError::InvalidSplitParent {
                        reason: super::SplitParentReason::TerminalParent,
                    });
                }
            }
            let resolved_depends_on = validate_task_dependencies(p, &tid, &task.depends_on)?;
            task.depends_on = resolved_depends_on.clone();
            super::ensure_group_declared(p, task.group.as_deref());
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
        self.close_task_rows(plan_id, task_id, result, from, None, now_ms)
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
        self.close_task_rows(plan_id, task_id, result, from, Some(authority), now_ms)
    }

    fn close_task_rows(
        &self,
        plan_id: &str,
        task_id: &str,
        result: Option<&str>,
        from: AgentRef,
        authority: Option<&crate::service_plan::interruption::TaskMutationAuthority>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        if let Some(result) = result {
            crate::service_plan::text_bounds::validate_plan_text("task result", result)?;
        }
        let milestone_id = self.mint_id("ms");
        self.with_task_rows(
            plan_id,
            task_id,
            crate::service_plan::store_db::hot_write::TaskHistory::Core,
            now_ms,
            |edit| {
                if let Some(authority) = authority {
                    crate::service_plan::interruption::authorize_task_mutation_for_owner(
                        edit.owner_tab.as_deref(),
                        &edit.task,
                        authority,
                    )?;
                }
                if edit.task.status == TaskStatus::Pending
                    && (authority.is_some()
                        || !(from.tab_id.is_some()
                            && from.tab_id == edit.owner_tab
                            && result.is_some_and(|note| !note.trim().is_empty())))
                {
                    return Err(PlanError::TransitionDenied {
                        from: TaskStatus::Pending,
                        to: TaskStatus::Done,
                    });
                }
                edit.terminal_check = true;
                edit.close_all_live = true;
                let task = &mut edit.task;
                if task.status == TaskStatus::InReview {
                    task.review_evidence.push(ReviewEvidence {
                        id: crate::plan::ids::short_id("rev"),
                        from: from.clone(),
                        verdict: ReviewVerdict::Accept,
                        note: result.unwrap_or("").into(),
                        at: now_ms,
                        changed_executable_surfaces: false,
                        distinct_oracle: false,
                    });
                    edit.review_credit = 1;
                }
                if task.status != TaskStatus::Done {
                    task.status_changed_at = now_ms;
                }
                task.status = TaskStatus::Done;
                task.updated_at = now_ms;
                super::qa::normalize_task_review_fields(task);
                clear_task_assignment(task);
                if let Some(text) = result {
                    task.milestones.push(Milestone {
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
            },
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
        crate::service_plan::text_bounds::validate_plan_label("milestone kind", kind)?;
        crate::service_plan::text_bounds::validate_plan_text("milestone text", text)?;
        let mid = self.mint_id("ms");
        self.commit_hot_changes(
            plan_id,
            vec![
                crate::service_plan::store_db::hot_write::HotChange::Milestone {
                    task_id: task_id.into(),
                    add_kind,
                    milestone: Milestone {
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
                    },
                },
            ],
            now_ms,
        )?;
        Ok(mid)
    }

    /// Append the narrow, idempotent checklist-link audit event. This method only
    /// mutates the task's milestone log; it does not alter task state or ownership.
    pub fn record_tab_todo_linked(
        &self,
        plan_id: &str,
        task_id: &str,
        todo_item_id: &str,
        label: &str,
        from_tab_id: &str,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        crate::service_plan::text_bounds::validate_plan_label("todo item id", todo_item_id)?;
        crate::service_plan::text_bounds::validate_plan_label("todo link label", label)?;
        let milestone_id = format!("tab_todo_{todo_item_id}");
        let text = format!("Checklist item linked: {label}");
        crate::service_plan::text_bounds::validate_plan_text("todo link event", &text)?;
        self.commit_hot_changes(
            plan_id,
            vec![
                crate::service_plan::store_db::hot_write::HotChange::AuditMilestone {
                    task_id: task_id.into(),
                    milestone: Milestone {
                        id: milestone_id.clone(),
                        from: AgentRef::from_tab(from_tab_id),
                        kind: "tab_todo_linked".into(),
                        text,
                        ts: now_ms,
                        links: vec![],
                        loop_id: None,
                        iteration: None,
                        canvas: Default::default(),
                        attachments: vec![],
                    },
                },
            ],
            now_ms,
        )?;
        Ok(milestone_id)
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
        crate::service_plan::text_bounds::validate_plan_label("stride kind", kind)?;
        crate::service_plan::text_bounds::validate_plan_text("stride text", text)?;
        let mid = self.mint_id("ms");
        self.commit_hot_changes(
            plan_id,
            vec![crate::service_plan::store_db::hot_write::HotChange::Stride(
                Milestone {
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
                },
            )],
            now_ms,
        )?;
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
        crate::service_plan::text_bounds::validate_plan_text("retrospective", text)?;
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
        let writer_turn = self.mutation_writer.enter();
        let plan = self.plan_identity_projection(plan_id)?;
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
        self.with_plan_in_writer(&writer_turn, plan_id, now_ms, |p| {
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
        if let Some(title) = title {
            crate::service_plan::text_bounds::validate_plan_label("section title", title)?;
        }
        if let Some(body) = body {
            crate::service_plan::text_bounds::validate_plan_text("section body", body)?;
        }
        for tag in tags {
            crate::service_plan::text_bounds::validate_plan_label("section tag", tag)?;
        }
        if let Some(primary) = primary {
            crate::service_plan::text_bounds::validate_plan_label("section primary tag", primary)?;
        }
        if let Some(handle) = handle {
            crate::service_plan::text_bounds::validate_plan_label("section handle", handle)?;
        }
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
        let removed = self.section_removal_ids_projection(plan_id, section_id)?;
        self.with_plan(plan_id, now_ms, |p| {
            if rm_section_tree(&mut p.sections, section_id) {
                Ok(())
            } else {
                Err(PlanError::NotFound)
            }
        })?;
        for section_id in removed {
            self.remove_section_attachment_dir(plan_id, &section_id)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod planning_state_tests;

#[cfg(test)]
mod team_binding_tests;

#[cfg(test)]
mod writer_queue_tests;
