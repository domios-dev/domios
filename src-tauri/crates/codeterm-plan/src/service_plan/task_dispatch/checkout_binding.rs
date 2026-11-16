use super::super::{PlanError, PlanStore};

impl PlanStore {
    pub fn record_attempt_checkout(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        checkout_path: String,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let git = |args: &[&str]| -> Result<String, PlanError> {
            let mut command = std::process::Command::new("git");
            codeterm_util::process_util::no_window_std(&mut command);
            let output = command
                .arg("-C")
                .arg(&checkout_path)
                .args(args)
                .env("GIT_OPTIONAL_LOCKS", "0")
                .output()
                .map_err(|error| PlanError::Io(error.to_string()))?;
            if !output.status.success() {
                return Err(PlanError::Io(format!(
                    "launch checkout inspection failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )));
            }
            String::from_utf8(output.stdout)
                .map(|value| value.trim().to_string())
                .map_err(|error| PlanError::Io(error.to_string()))
        };
        if !std::path::Path::new(&checkout_path).is_dir() {
            return Err(PlanError::Io(format!(
                "launch checkout '{checkout_path}' is not an existing directory"
            )));
        }
        // A folder without git history binds the folder itself and has no start commit.
        let (root, head) = match super::super::shared_checkout::project_history(&checkout_path)
            .map_err(PlanError::Io)?
        {
            super::super::shared_checkout::ProjectHistory::NoGit => (checkout_path.clone(), None),
            super::super::shared_checkout::ProjectHistory::Git => (
                git(&["rev-parse", "--show-toplevel"])?,
                Some(git(&["rev-parse", "--verify", "HEAD^{commit}"])?),
            ),
        };
        // Stored with its real case like worktree_path; compared only by key.
        let checkout_path = codeterm_paths::canonical_cwd(&root);
        let checkout_key = super::super::shared_checkout::canonical_path(&checkout_path);
        self.with_task_rows(
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| {
                        attempt.id == attempt_id
                            && attempt.executor_tab_id == tab_id
                            && attempt.closed_at.is_none()
                            && attempt.retired_at.is_none()
                    })
                    .ok_or_else(|| {
                        Self::task_readback_error("attempt_checkout", "live attempt missing")
                    })?;
                if attempt.checkout_path.as_ref().is_some_and(|bound| {
                    super::super::shared_checkout::canonical_path(bound) != checkout_key
                }) {
                    return Err(Self::task_readback_error(
                        "attempt_checkout",
                        "checkout binding changed",
                    ));
                }
                attempt.checkout_path = Some(checkout_path);
                if attempt.shared_start_sha.is_none() && attempt.shared_start_files.is_none() {
                    attempt.shared_start_sha = head;
                }
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }
}
