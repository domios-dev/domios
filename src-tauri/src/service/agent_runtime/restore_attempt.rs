use crate::{multiplexer::types::TaskBinding, plan::TaskExecutionAttempt, service::ServiceContext};

#[derive(Debug, PartialEq, Eq)]
enum AttemptResolutionRefusal {
    Missing {
        task: String,
        candidates: Vec<String>,
    },
    Ambiguous {
        task: String,
        candidates: Vec<String>,
    },
}

impl std::fmt::Display for AttemptResolutionRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (code, task, candidates) = match self {
            Self::Missing { task, candidates } => {
                ("runtime_restore_attempt_missing", task, candidates)
            }
            Self::Ambiguous { task, candidates } => {
                ("runtime_restore_attempt_ambiguous", task, candidates)
            }
        };
        write!(f, "{code}: task={task}, candidates={candidates:?}")
    }
}

fn resolve_open_attempt<'a>(
    task: &str,
    tab: &str,
    attempts: &'a [TaskExecutionAttempt],
) -> Result<&'a str, AttemptResolutionRefusal> {
    let matches = attempts
        .iter()
        .filter(|attempt| {
            attempt.executor_tab_id == tab
                && attempt.closed_at.is_none()
                && attempt.retired_at.is_none()
        })
        .collect::<Vec<_>>();
    let candidates = matches.iter().map(|attempt| attempt.id.clone()).collect();
    match matches.as_slice() {
        [attempt] => Ok(&attempt.id),
        [] => Err(AttemptResolutionRefusal::Missing {
            task: task.into(),
            candidates,
        }),
        _ => Err(AttemptResolutionRefusal::Ambiguous {
            task: task.into(),
            candidates,
        }),
    }
}

impl ServiceContext {
    pub(crate) fn restore_runtime_attempt_binding(&self, tab_id: &str) -> Result<(), String> {
        let target = self
            .mux
            .read()
            .get_tab(tab_id)
            .cloned()
            .ok_or("Runtime tab disappeared")?;
        if !target.restored_from_snapshot {
            return Ok(());
        }
        let Some(binding) = target
            .task_binding
            .as_ref()
            .filter(|binding| binding.attempt_id.is_none())
        else {
            return Ok(());
        };
        let (Some(plan), Some(task)) = (binding.plan_id.as_deref(), binding.task_id.as_deref())
        else {
            return Ok(());
        };
        let detail = self.plan_store.show_task(task).ok_or_else(|| {
            AttemptResolutionRefusal::Missing {
                task: task.into(),
                candidates: vec![],
            }
            .to_string()
        })?;
        if detail.plan_id != plan {
            return Err("Runtime restored task plan changed".into());
        }
        let attempt = resolve_open_attempt(task, tab_id, &detail.task.execution_attempts)
            .map_err(|reason| reason.to_string())?;
        let mut mux = self.mux.write();
        let current = mux.get_tab_mut(tab_id).ok_or("Runtime tab disappeared")?;
        let resolved = TaskBinding {
            attempt_id: Some(attempt.into()),
            ..binding.clone()
        };
        if !current.restored_from_snapshot
            || current.agent_type != target.agent_type
            || current.agent_session_id != target.agent_session_id
            || current.session_binding_nonce != target.session_binding_nonce
            || (current.task_binding.as_ref() != Some(binding)
                && current.task_binding.as_ref() != Some(&resolved))
        {
            return Err("Runtime restored task binding changed".into());
        }
        current.task_binding = Some(resolved);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(id: &str, tab: &str) -> TaskExecutionAttempt {
        TaskExecutionAttempt {
            id: id.into(),
            executor_tab_id: tab.into(),
            ..Default::default()
        }
    }

    #[test]
    fn single_open_attempt_resolves_only_its_executor() {
        let attempts = [attempt("foreign", "other"), attempt("own", "tab")];
        assert_eq!(resolve_open_attempt("task", "tab", &attempts), Ok("own"));
    }

    #[test]
    fn no_open_attempt_refuses_with_task_and_empty_candidates() {
        assert_eq!(
            resolve_open_attempt("task", "tab", &[attempt("foreign", "other")]),
            Err(AttemptResolutionRefusal::Missing {
                task: "task".into(),
                candidates: vec![]
            })
        );
    }

    #[test]
    fn several_open_attempts_refuse_with_all_candidates() {
        let attempts = [attempt("first", "tab"), attempt("second", "tab")];
        assert_eq!(
            resolve_open_attempt("task", "tab", &attempts),
            Err(AttemptResolutionRefusal::Ambiguous {
                task: "task".into(),
                candidates: vec!["first".into(), "second".into()]
            })
        );
    }

    #[test]
    fn closed_and_retired_attempts_are_ignored() {
        let mut closed = attempt("closed", "tab");
        closed.closed_at = Some(1);
        let mut retired = attempt("retired", "tab");
        retired.retired_at = Some(1);
        let mut attempts = vec![closed, retired];
        assert!(matches!(
            resolve_open_attempt("task", "tab", &attempts),
            Err(AttemptResolutionRefusal::Missing { .. })
        ));
        attempts.push(attempt("open", "tab"));
        assert_eq!(resolve_open_attempt("task", "tab", &attempts), Ok("open"));
    }
}
