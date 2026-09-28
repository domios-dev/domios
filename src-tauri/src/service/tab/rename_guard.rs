use crate::multiplexer::types::TaskBinding;

/// Prefix on every task-bound rename refusal so transports can classify it.
pub(crate) const TASK_BOUND_RENAME_REFUSAL_PREFIX: &str = "task-bound rename refused";

pub(crate) fn refuse_task_bound_rename(
    tab_id: &str,
    task_binding: Option<&TaskBinding>,
) -> Result<(), String> {
    if task_binding.is_some() {
        return Err(format!(
            "{TASK_BOUND_RENAME_REFUSAL_PREFIX}: Tab '{tab_id}' keeps its task-bound identity and planned name; use `codeterm tab describe` for progress"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{refuse_task_bound_rename, TASK_BOUND_RENAME_REFUSAL_PREFIX};
    use crate::multiplexer::types::TaskBinding;

    #[test]
    fn tab_name_contract_task_bound_rename_guard_remains_refused() {
        let binding = TaskBinding {
            plan_id: Some("plan".into()),
            task_id: Some("task".into()),
            attempt_id: None,
            ..Default::default()
        };

        let error = refuse_task_bound_rename("tab", Some(&binding)).expect_err("rename refused");

        assert!(error.starts_with(TASK_BOUND_RENAME_REFUSAL_PREFIX));
        assert!(error.contains("task-bound identity"));
        assert!(error.contains("tab describe"));
    }

    #[test]
    fn tab_name_contract_unbound_tab_rename_remains_allowed() {
        assert!(refuse_task_bound_rename("tab", None).is_ok());
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn task_bound_tab_rename_is_refused() {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        if let Some(tab) = mux.get_tab_mut(&tab_id) {
            tab.task_binding = Some(TaskBinding {
                task_id: Some("task".into()),
                attempt_id: None,
                ..Default::default()
            });
        }
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);

        let error = fixture
            .service
            .rename_tab(&tab_id, "wrong-name")
            .expect_err("task-bound rename must be refused");

        assert!(error.starts_with(TASK_BOUND_RENAME_REFUSAL_PREFIX));
        assert!(error.contains("task-bound identity"));
    }
}
