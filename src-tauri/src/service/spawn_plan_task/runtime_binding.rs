use super::*;

impl ServiceContext {
    pub(crate) fn prepare_runtime_worker_binding(
        &self,
        params: &SpawnAgentParams,
        tab_id: &str,
    ) -> Result<(), String> {
        let mut prepared = params.clone();
        if prepared.remote_task_binding.is_none()
            && prepared.plan_task.is_none()
            && prepared.task_launch_authority.is_some()
        {
            prepared.plan_task = prepared.task_id.clone();
        }
        self.bind_worker_plan_task_or_rollback_tab(&prepared, tab_id)
    }

    pub(crate) fn finalize_runtime_worker_session(
        &self,
        tab_id: &str,
        thread: &str,
        nonce: &str,
    ) -> Result<(), String> {
        let (binding, restored, remote) = self
            .mux
            .read()
            .get_tab(tab_id)
            .map(|tab| {
                (
                    tab.task_binding.clone(),
                    tab.restored_from_snapshot,
                    tab.report_target_mesh_peer.is_some(),
                )
            })
            .unwrap_or_default();
        let Some(binding) = binding else {
            return Ok(());
        };
        let (Some(plan), Some(task), Some(attempt)) =
            (binding.plan_id, binding.task_id, binding.attempt_id)
        else {
            return Err("Runtime task binding incomplete".into());
        };
        let Some(detail) = self.plan_store.show_task(&task) else {
            return if remote {
                Ok(())
            } else {
                Err("Runtime task disappeared before readiness".into())
            };
        };
        if detail.plan_id != plan {
            return Err("Runtime task plan changed".into());
        }
        let existing = detail
            .task
            .execution_attempts
            .iter()
            .find(|entry| entry.id == attempt && entry.executor_tab_id == tab_id)
            .ok_or("Runtime task attempt changed")?;
        if needs_attempt_finalization(
            existing.session_id.as_deref(),
            existing.session_binding_nonce.as_deref(),
            existing.closed_at.is_some(),
            existing.retired_at.is_some(),
            restored,
            thread,
            nonce,
        )? {
            self.plan_store
                .finish_task_dispatch(
                    &plan,
                    &task,
                    tab_id,
                    &attempt,
                    thread,
                    nonce,
                    detail.task.lane,
                    epoch_ms(),
                )
                .map_err(|reason| reason.to_string())?;
        }
        Ok(())
    }
}

fn needs_attempt_finalization(
    saved_thread: Option<&str>,
    saved_nonce: Option<&str>,
    closed: bool,
    retired: bool,
    restored: bool,
    thread: &str,
    nonce: &str,
) -> Result<bool, String> {
    if retired {
        return Err("Runtime task attempt retired".into());
    }
    if saved_thread.is_some_and(|bound| bound != thread)
        || (!restored && saved_nonce.is_some_and(|bound| bound != nonce))
    {
        return Err("Runtime task session rebound".into());
    }
    if closed {
        return if saved_thread == Some(thread) {
            Ok(false)
        } else {
            Err("Closed runtime attempt has no bound conversation".into())
        };
    }
    Ok(saved_thread.is_none() || saved_nonce != Some(nonce))
}

#[cfg(test)]
mod tests {
    use super::super::tests::store_with_task;
    use super::*;
    #[test]
    fn completed_attempt_resumes_only_its_conversation_without_reopening_execution() {
        assert_eq!(
            needs_attempt_finalization(Some("own"), Some("old"), true, false, true, "own", "new"),
            Ok(false)
        );
        assert!(needs_attempt_finalization(
            Some("other"),
            Some("old"),
            true,
            false,
            true,
            "own",
            "new"
        )
        .is_err());
        assert!(
            needs_attempt_finalization(None, Some("old"), true, false, true, "own", "new").is_err()
        );
        assert!(needs_attempt_finalization(
            Some("own"),
            Some("old"),
            true,
            false,
            false,
            "own",
            "new"
        )
        .is_err());
        assert!(needs_attempt_finalization(
            Some("own"),
            Some("old"),
            true,
            true,
            true,
            "own",
            "new"
        )
        .is_err());
        assert_eq!(
            needs_attempt_finalization(Some("own"), Some("old"), false, false, true, "own", "new"),
            Ok(true)
        );
        assert_eq!(
            needs_attempt_finalization(
                Some("own"),
                Some("same"),
                false,
                false,
                false,
                "own",
                "same"
            ),
            Ok(false)
        );
    }
    #[test]
    fn prepared_task_attempt_is_committed_once_before_runtime_and_finalized_on_exact_session() {
        let dir = tempfile::tempdir().unwrap();
        let (store, plan, task) = store_with_task(dir.path());
        let bind = |session, nonce| {
            bind_worker_task_to_tab(
                &store,
                Some(&plan),
                "spawn-bind",
                "tab-worker",
                "Ada",
                session,
                nonce,
                crate::plan::ExecutionIntent::ReadOnly,
                None,
                Some("fixture-provider"),
                None,
                None,
                None,
                "attempt-prepared",
                None,
                Vec::new(),
                10,
            )
        };
        bind(None, None).unwrap();
        bind(None, None).unwrap();
        bind(Some("own-session"), Some("own-nonce")).unwrap();
        bind(Some("own-session"), Some("own-nonce")).unwrap();
        assert!(bind(Some("foreign-session"), Some("own-nonce")).is_err());
        let task = store.show_task(&task).unwrap().task;
        assert_eq!(task.execution_attempts.len(), 1);
        assert_eq!(task.execution_attempts[0].id, "attempt-prepared");
        assert_eq!(
            task.execution_attempts[0].session_id.as_deref(),
            Some("own-session")
        );
        assert_eq!(
            task.execution_attempts[0].session_binding_nonce.as_deref(),
            Some("own-nonce")
        );
    }
}
