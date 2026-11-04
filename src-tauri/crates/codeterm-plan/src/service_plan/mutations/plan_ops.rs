use super::*;
use crate::service_plan::store_db::hot_write::HeaderChange;
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
        let edit = crate::service_plan::store_db::hot_write::PlanEdit {
            title: title.map(str::to_string),
            status,
            mode: mode.map(parse_plan_mode).transpose()?,
            ..Default::default()
        };
        self.commit_plan_state(plan_id, edit, now_ms)
    }

    pub fn set_planner_ref(
        &self,
        plan_id: &str,
        planner_ref: Option<PlannerRef>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.commit_header_change(plan_id, HeaderChange::Planner(planner_ref), now_ms)
            .map(|_| ())
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
        self.commit_header_change(plan_id, HeaderChange::Mode(mode), now_ms)
            .map(|_| ())
    }

    pub fn set_planning_state(
        &self,
        plan_id: &str,
        to: PlanningState,
        by: PlanningAuthor,
        note: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.commit_header_change(
            plan_id,
            HeaderChange::Planning {
                to,
                by,
                note: note
                    .map(str::trim)
                    .filter(|note| !note.is_empty())
                    .map(str::to_string),
            },
            now_ms,
        )
        .map(|_| ())
    }

    pub fn set_kinds(
        &self,
        plan_id: &str,
        kinds: Vec<MilestoneKind>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.commit_header_change(plan_id, HeaderChange::Kinds(kinds), now_ms)
            .map(|_| ())
    }

    pub fn set_task_groups(
        &self,
        plan_id: &str,
        groups: Vec<TaskGroup>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.commit_header_change(plan_id, HeaderChange::Groups(groups), now_ms)
            .map(|_| ())
    }

    pub fn set_tag_facets(
        &self,
        plan_id: &str,
        facets: Vec<TagFacet>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.commit_header_change(plan_id, HeaderChange::Facets(facets), now_ms)
            .map(|_| ())
    }

    pub fn link_plan(&self, plan_id: &str, to: &str, now_ms: u64) -> Result<(), PlanError> {
        self.commit_header_change(
            plan_id,
            HeaderChange::Link {
                to: to.into(),
                remove: false,
            },
            now_ms,
        )
        .map(|_| ())
    }

    pub fn unlink_plan(&self, plan_id: &str, to: &str, now_ms: u64) -> Result<(), PlanError> {
        self.commit_header_change(
            plan_id,
            HeaderChange::Link {
                to: to.into(),
                remove: true,
            },
            now_ms,
        )
        .map(|_| ())
    }

    pub fn owner_of(&self, id: &str) -> Option<OrchestratorRef> {
        self.plan_identity_projection(id)
            .ok()
            .map(|plan| plan.owner)
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
        let sid = self.mint_id("sec")?;
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
        self.commit_section_change(
            plan_id,
            crate::service_plan::store_db::hot_write::SectionChange::Add {
                section,
                parent: parent.map(str::to_string),
            },
            now_ms,
        )?;
        Ok(sid)
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
        let milestone_id = self.mint_id("ms")?;
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
        let mid = self.mint_id("ms")?;
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
        let mid = self.mint_id("ms")?;
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
        let mid = self.mint_id("ms")?;
        let supersede_id = self.mint_id("ms")?;
        let turn = self.mutation_writer.enter();
        let receipt = self.database.append_plan_audit(
            plan_id,
            Milestone {
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
            },
            crate::service_plan::store_db::hot_write::PlanAudit::Retrospective { supersede_id },
            now_ms,
        )?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(mid)
    }

    pub fn archive(&self, plan_id: &str, now_ms: u64) -> Result<(), PlanError> {
        self.archive_with_incident(plan_id, None, now_ms)
    }

    pub fn archive_with_incident(
        &self,
        plan_id: &str,
        incident: Option<PlanIncident>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let incident = if let Some(incident) = incident {
            if incident.kind.trim().is_empty() || incident.text.trim().is_empty() {
                return Err(PlanError::Io(
                    "plan archive incident requires a kind and text".into(),
                ));
            }
            Some(Milestone {
                id: self.mint_id("ms")?,
                from: incident.from,
                kind: incident.kind,
                text: incident.text,
                ts: now_ms,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            })
        } else {
            None
        };
        self.commit_plan_state(
            plan_id,
            crate::service_plan::store_db::hot_write::PlanEdit {
                status: Some(PlanStatus::Archived),
                incident,
                ..Default::default()
            },
            now_ms,
        )
    }

    pub fn unarchive(&self, plan_id: &str, now_ms: u64) -> Result<(), PlanError> {
        self.commit_plan_state(
            plan_id,
            crate::service_plan::store_db::hot_write::PlanEdit {
                status: Some(PlanStatus::Active),
                unarchive: true,
                ..Default::default()
            },
            now_ms,
        )
    }

    fn commit_plan_state(
        &self,
        plan_id: &str,
        edit: crate::service_plan::store_db::hot_write::PlanEdit,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let turn = self.mutation_writer.enter();
        #[cfg(test)]
        if let (Some(hook), Some(title)) =
            (self.persistence_hook.read().clone(), edit.title.as_deref())
        {
            hook(title);
        }
        let receipt = self.database.edit_plan_state(plan_id, edit, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(())
    }

    pub fn transfer(
        &self,
        plan_id: &str,
        to: OrchestratorRef,
        reason: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.commit_header_change(
            plan_id,
            HeaderChange::Owner {
                to,
                reason: reason.into(),
            },
            now_ms,
        )
        .map(|_| ())
    }

    pub fn refresh_owner_session(
        &self,
        plan_id: &str,
        tab_id: &str,
        session_id: Option<&str>,
        title: Option<&str>,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.commit_header_change(
            plan_id,
            HeaderChange::RefreshOwner {
                tab_id: tab_id.into(),
                session_id: session_id.map(str::to_string),
                title: title.map(str::to_string),
            },
            now_ms,
        )
    }

    pub fn disown(&self, plan_id: &str, now_ms: u64) -> Result<(), PlanError> {
        self.transfer(plan_id, OrchestratorRef::default(), "disown", now_ms)
    }

    pub fn claim(
        &self,
        plan_id: &str,
        new_owner: OrchestratorRef,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.transfer(plan_id, new_owner, "claim", now_ms)
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
        self.commit_section_change(
            plan_id,
            crate::service_plan::store_db::hot_write::SectionChange::Update {
                selector: section_id.into(),
                title: title.map(str::to_string),
                body: body.map(str::to_string),
                tags: tags.to_vec(),
                primary: primary.map(str::to_string),
                handle: handle.map(str::to_string),
            },
            now_ms,
        )
        .map(|_| ())
    }

    pub fn rm_section(
        &self,
        plan_id: &str,
        section_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let removed = self.commit_section_change(
            plan_id,
            crate::service_plan::store_db::hot_write::SectionChange::Remove {
                selector: section_id.into(),
            },
            now_ms,
        )?;
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
