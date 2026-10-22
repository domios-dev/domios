use super::{now_secs, ServiceContext, TabIdentity};
use codeterm_types::agent::{
    plan_tab_todo_write, validate_tab_todo_reason, TabTodoChecklist, TabTodoNewItem,
    TabTodoProposal, TabTodoWriteEntry,
};

impl ServiceContext {
    pub(super) fn require_checklist_reviewer(
        &self,
        target: &TabIdentity,
        actor: Option<&str>,
    ) -> Result<(), String> {
        let Some(actor) = actor else {
            return Ok(());
        };
        self.todo_tab(actor)?;
        let owner = target
            .plan_id
            .clone()
            .or_else(|| {
                self.plan_store
                    .task_binding_for_tab(&target.id)
                    .map(|(id, _)| id)
            })
            .and_then(|id| self.plan_store.get(&id))
            .and_then(|plan| plan.owner.tab_id);
        if codeterm_types::agent::can_review_tab_todo(
            actor,
            target.parent_tab_id.as_deref(),
            owner.as_deref(),
        ) {
            Ok(())
        } else {
            Err("only the Worker's parent or Plan owner may review its checklist".into())
        }
    }

    pub async fn tab_todo_submit(
        &self,
        target: &str,
        actor: &str,
    ) -> Result<TabTodoChecklist, String> {
        let target = self.todo_tab(target)?;
        self.require_owner_or_user(&target, Some(actor))?;
        let store = self.todo_store()?;
        let current = store
            .get(target.id.clone())
            .await?
            .ok_or_else(|| "write a checklist before sharing it".to_string())?;
        if current.proposal.is_some() {
            return Ok(current);
        }
        let checklist = store
            .review_proposal(
                &current,
                TabTodoProposal::shared(now_secs()),
                vec![],
                None,
                Some(actor.into()),
                now_secs(),
            )
            .await?;
        let checklist = self.todo_changed(checklist);
        if let Some(parent) = &target.parent_tab_id {
            let items = checklist
                .items
                .iter()
                .map(|item| {
                    format!(
                        "- [{}] {}",
                        if item.locked { "criterion" } else { "plan" },
                        item.title
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let text = format!("Checklist shared by {} (work continues):\n{}\nReview: domios todo approve --tab {} or domios todo amend --tab {} --from-pad checklist.json", target.id, items, target.id, target.id);
            self.deliver_checklist_notice(parent, &text).await;
        }
        self.ui.emit("tab-todo-proposed", serde_json::json!({ "tab_id": target.id, "parent_tab_id": target.parent_tab_id, "checklist": checklist }));
        Ok(checklist)
    }

    pub async fn tab_todo_review_proposal(
        &self,
        target: &str,
        actor: Option<&str>,
        entries: Option<Vec<TabTodoWriteEntry>>,
        note: Option<String>,
        expected: Option<(u64, u64)>,
    ) -> Result<TabTodoChecklist, String> {
        let target = self.todo_tab(target)?;
        self.require_checklist_reviewer(&target, actor)?;
        if let Some(note) = &note {
            validate_tab_todo_reason(note)?;
        }
        let store = self.todo_store()?;
        let current = store
            .get(target.id.clone())
            .await?
            .ok_or_else(|| "Tab has no checklist".to_string())?;
        if expected.is_some_and(|(generation, revision)| {
            generation != current.generation || revision != current.revision
        }) {
            return Err("checklist changed; reopen the review before saving".into());
        }
        let mut proposal = current
            .proposal
            .clone()
            .unwrap_or_else(|| TabTodoProposal::shared(now_secs()));
        proposal.review(
            entries.is_some(),
            actor.map(str::to_owned),
            note,
            now_secs(),
        );
        let mut amendments = Vec::new();
        let mut revised = current.clone();
        let plan = if let Some(entries) = entries {
            for entry in &entries {
                entry.validate()?;
                if let Some(item) = revised
                    .items
                    .iter_mut()
                    .find(|item| entry.id.as_deref() == Some(&item.id))
                {
                    if !entry.content.is_empty() && entry.content != item.title {
                        item.validate_amendment_title(&entry.content)?;
                        amendments.push((
                            item.id.clone(),
                            item.revision,
                            TabTodoNewItem {
                                id: item.id.clone(),
                                title: entry.content.clone(),
                                description: item.description.clone(),
                                link: item.link.clone(),
                            },
                        ));
                        item.title = entry.content.clone();
                        item.revision += 1;
                    }
                }
            }
            Some(plan_tab_todo_write(Some(&revised), &entries)?)
        } else {
            None
        };
        let mut appended = Vec::new();
        if let Some(plan) = &plan {
            for entry in &plan.appends {
                appended.push(
                    self.resolve_draft(uuid::Uuid::new_v4().to_string(), entry.draft())
                        .await?,
                );
            }
        }
        let checklist = store
            .review_proposal(
                &current,
                proposal,
                amendments,
                plan.as_ref().map(|plan| (plan, appended)),
                actor.map(str::to_owned),
                now_secs(),
            )
            .await?;
        let checklist = self.todo_changed(checklist);
        let changes = checklist
            .items
            .iter()
            .filter_map(
                |item| match current.items.iter().find(|old| old.id == item.id) {
                    None => Some(format!("+ {}", item.title)),
                    Some(old) if old.title != item.title => {
                        Some(format!("~ {} -> {}", old.title, item.title))
                    }
                    Some(old) if old.state != item.state => Some(format!(
                        "~ {}: {} -> {}",
                        item.title,
                        old.state.label(),
                        item.state.label()
                    )),
                    _ => None,
                },
            )
            .chain(
                current
                    .items
                    .iter()
                    .filter(|old| !checklist.items.iter().any(|item| item.id == old.id))
                    .map(|old| format!("- {}", old.title)),
            )
            .collect::<Vec<_>>()
            .join("\n");
        let proposal = checklist
            .proposal
            .as_ref()
            .ok_or_else(|| "review missing proposal".to_string())?;
        self.deliver_checklist_notice(
            &target.id,
            &format!(
                "Checklist {}. {}\n{}",
                proposal.state.label(),
                proposal.note.as_deref().unwrap_or(""),
                changes
            ),
        )
        .await;
        self.ui.emit(
            "tab-todo-reviewed",
            serde_json::json!({ "tab_id": target.id, "proposal": proposal }),
        );
        Ok(checklist)
    }

    async fn deliver_checklist_notice(&self, target: &str, text: &str) {
        if let Err(error) = self.deliver_checklist_text(target, text).await {
            log::warn!("[tab-todo] checklist notice unavailable for {target}: {error}");
        }
    }

    pub(super) async fn deliver_checklist_text(
        &self,
        target: &str,
        text: &str,
    ) -> Result<bool, String> {
        let svc = self.clone();
        let target = target.to_owned();
        let text = text.to_owned();
        crate::service::blocking_run(move || {
            crate::service::hierarchy::deliver_tab_todo_recitation(&svc, &target, &text)
        })
        .await
    }
}
