use super::plan_warning::PlanBindingFailure;
use crate::service::plan::{PlanError, RuntimeAttemptRestoreQuery};
use crate::{multiplexer::types::TaskBinding, service::ServiceContext};

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
    candidates: &'a [String],
) -> Result<&'a str, AttemptResolutionRefusal> {
    match candidates {
        [attempt] => Ok(attempt),
        [] => Err(AttemptResolutionRefusal::Missing {
            task: task.into(),
            candidates: candidates.to_vec(),
        }),
        _ => Err(AttemptResolutionRefusal::Ambiguous {
            task: task.into(),
            candidates: candidates.to_vec(),
        }),
    }
}

impl ServiceContext {
    pub(crate) fn restore_runtime_attempt_binding(
        &self,
        tab_id: &str,
    ) -> Result<(), PlanBindingFailure> {
        self.restore_attempt_binding(tab_id, false)
    }

    pub(crate) fn restore_restart_attempt_binding(
        &self,
        tab_id: &str,
    ) -> Result<(), PlanBindingFailure> {
        self.restore_attempt_binding(tab_id, true)
    }

    fn restore_attempt_binding(
        &self,
        tab_id: &str,
        for_restart: bool,
    ) -> Result<(), PlanBindingFailure> {
        let target = self
            .mux
            .read()
            .get_tab(tab_id)
            .cloned()
            .ok_or("Runtime tab disappeared")?;
        if !target.restored_from_snapshot && !for_restart {
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
        let detail = self
            .plan_store
            .runtime_attempt_restore_projection(&RuntimeAttemptRestoreQuery {
                task_ref: task,
                executor_tab_id: tab_id,
                provider: target.agent_type.as_deref(),
                session_id: target.agent_session_id.as_deref(),
                nonce: target.session_binding_nonce.as_deref(),
                reported_nonce: target.reported_session_binding_nonce.as_deref(),
            })
            .map_err(|error| match error {
                PlanError::TaskNotFound => {
                    PlanBindingFailure::bookkeeping(AttemptResolutionRefusal::Missing {
                        task: task.into(),
                        candidates: vec![],
                    })
                }
                error => PlanBindingFailure::from_plan_error(error),
            })?;
        if detail.plan_id != plan {
            return Err("Runtime restored task plan changed".into());
        }
        let attempt = resolve_open_attempt(task, &detail.candidates)
            .map_err(|reason| PlanBindingFailure::from(reason.to_string()))?;
        if for_restart && detail.restart_acknowledged_at.is_none() {
            return Err("Restart attempt does not match the acknowledged source session".into());
        }
        let mut mux = self.mux.write();
        let current = mux.get_tab_mut(tab_id).ok_or("Runtime tab disappeared")?;
        let resolved = TaskBinding {
            attempt_id: Some(attempt.into()),
            ..binding.clone()
        };
        if (!current.restored_from_snapshot && !for_restart)
            || current.agent_type != target.agent_type
            || current.agent_session_id != target.agent_session_id
            || current.session_binding_nonce != target.session_binding_nonce
            || (for_restart
                && current.reported_session_binding_nonce != target.reported_session_binding_nonce)
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
mod tests;
