use super::{now_secs, ServiceContext};
use codeterm_types::agent::TabTodoState;

impl ServiceContext {
    pub(crate) async fn tab_todo_report_warning(
        &self,
        tab_id: &str,
    ) -> Result<Option<String>, String> {
        let Some(checklist) = self.todo_store()?.get(tab_id.to_owned()).await? else {
            return Ok(None);
        };
        let open: Vec<_> = checklist
            .items
            .iter()
            .filter(|item| item.locked && !item.state.is_terminal())
            .map(|item| format!("{}: {}", item.id, item.title))
            .collect();
        Ok((!open.is_empty()).then(|| format!(
            "WARNING: report accepted with open locked criteria: {}. Update them by id with codeterm todo write.",
            open.join("; ")
        )))
    }

    pub async fn tab_todo_apply_review(
        &self,
        tab_id: &str,
        actor_tab_id: Option<&str>,
        accepted: bool,
    ) -> Result<(), String> {
        let store = self.todo_store()?;
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
