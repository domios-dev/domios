use super::{now_secs, ServiceContext};
use codeterm_types::agent::TabTodoState;

impl ServiceContext {
    pub(crate) async fn tab_todo_report_warning(
        &self,
        tab_id: &str,
    ) -> Result<Option<String>, String> {
        let Some(store) = self.optional_todo_store() else {
            return Ok(None);
        };
        let Some(checklist) = store.get(tab_id.to_owned()).await? else {
            return Ok(None);
        };
        let open: Vec<_> = checklist
            .items
            .iter()
            .filter(|item| item.locked && !item.state.is_terminal())
            .map(|item| format!("{}: {}", item.id, item.title))
            .collect();
        if !open.is_empty() {
            return Ok(Some(format!(
            "WARNING: report accepted with open locked criteria: {}. Update them by id with codeterm todo write.",
            open.join("; ")
        )));
        }
        if checklist.proposal.is_none() && self.todo_tab(tab_id)?.parent_tab_id.is_some() {
            return Ok(Some(
                "hint: checklist was never shared; use codeterm todo submit".into(),
            ));
        }
        let remaining = checklist
            .items
            .iter()
            .filter(|item| !item.state.is_terminal())
            .count();
        Ok((remaining > 0).then(|| {
            format!(
                "hint: checklist has {remaining} open item(s); update it with codeterm todo write"
            )
        }))
    }

    pub async fn tab_todo_apply_review(
        &self,
        tab_id: &str,
        actor_tab_id: Option<&str>,
        accepted: bool,
    ) -> Result<(), String> {
        let Some(store) = self.optional_todo_store() else {
            return Ok(());
        };
        let Some(checklist) = store.get(tab_id.to_owned()).await? else {
            return Ok(());
        };
        for item in checklist.items.iter().filter(|item| item.locked) {
            if accepted && !item.state.is_terminal() {
                let updated = store
                    .set_state(
                        tab_id.to_owned(),
                        checklist.generation,
                        item.id.clone(),
                        item.revision,
                        TabTodoState::Done,
                        Some("accepted by review".into()),
                        actor_tab_id.map(str::to_owned),
                        now_secs(),
                    )
                    .await?;
                self.todo_changed(updated);
            } else if !accepted && item.state != TabTodoState::Open {
                let updated = store
                    .reopen(
                        tab_id.to_owned(),
                        checklist.generation,
                        item.id.clone(),
                        "reopened by review".into(),
                        actor_tab_id.map(str::to_owned),
                        now_secs(),
                    )
                    .await?;
                self.todo_changed(updated);
            }
        }
        Ok(())
    }
}
