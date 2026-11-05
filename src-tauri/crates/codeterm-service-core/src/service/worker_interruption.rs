use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use crate::plan::InterruptionCheckpoint;
use crate::service::plan::interruption::checkpoint_digest;

const CHECKPOINT_ARTIFACT_MAX_BYTES: usize = 4 * 1024 * 1024;
const CHECKPOINT_UNTRACKED_FILE_READ_LIMIT: u64 = CHECKPOINT_ARTIFACT_MAX_BYTES as u64;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerInterruptionEvent {
    ProviderExhausted,
    ProcessExited,
    ForcedContextReset,
}

fn pending_interruptions() -> &'static Mutex<std::collections::HashSet<String>> {
    static PENDING: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

impl WorkerInterruptionEvent {
    pub fn reason(self) -> &'static str {
        match self {
            Self::ProviderExhausted => "provider_exhausted",
            Self::ProcessExited => "process_exited",
            Self::ForcedContextReset => "forced_context_reset",
        }
    }
}

pub struct CheckpointRequest<'a> {
    pub task_id: &'a str,
    pub attempt_id: &'a str,
    pub tab_id: &'a str,
    pub session_id: &'a str,
    pub owner_epoch: u64,
    pub repo_path: &'a Path,
    pub worktree_path: &'a Path,
    pub base_head: &'a str,
    pub completed_tests: Vec<String>,
    pub unresolved_work: String,
    pub next_acceptance_gate: String,
}

#[derive(Debug, Clone)]
pub struct WorkerInterruptionRequest {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub tab_id: String,
    pub session_id: String,
    pub owner_epoch: u64,
    pub repo_path: PathBuf,
    pub worktree_path: PathBuf,
    pub base_head: String,
    pub completed_tests: Vec<String>,
    pub unresolved_work: String,
    pub next_acceptance_gate: String,
    pub event: WorkerInterruptionEvent,
    pub occurred_at: u64,
}

impl super::ServiceContext {
    pub fn verify_task_owner_session(
        &self,
        task_id: &str,
        tab_id: &str,
        session_id: &str,
        owner_epoch: u64,
    ) -> Result<(), String> {
        let authenticated = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.agent_session_id.as_deref() == Some(session_id)
                && tab.session_binding_nonce.as_ref().is_some_and(|nonce| {
                    tab.reported_session_binding_nonce.as_deref() == Some(nonce.as_str())
                })
        });
        if !authenticated {
            return Err("task owner session binding is stale or unauthenticated".into());
        }
        self.plan_store
            .verify_task_owner_epoch(task_id, tab_id, session_id, owner_epoch)
            .map_err(|error| error.to_string())
    }

    pub fn signal_provider_exhaustion(
        &self,
        provider: &str,
    ) -> Result<usize, crate::service::plan::PlanError> {
        let tabs = self
            .plan_store
            .provider_exhaustion_tabs_projection(provider)?;
        let mut interrupted = 0;
        for tab in tabs {
            if self.signal_worker_interruption(&tab, WorkerInterruptionEvent::ProviderExhausted)? {
                interrupted += 1;
            }
        }
        Ok(interrupted)
    }

    pub fn signal_provider_recovery(
        &self,
        provider: &str,
    ) -> Result<usize, crate::service::plan::PlanError> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let targets = self.plan_store.provider_recovery_projection(provider)?;
        let mut resumed = 0;
        for target in targets {
            self.plan_store.resume_from_provider_exhaustion(
                &target.plan_id,
                &target.task_id,
                &target.owner_tab_id,
                &target.owner_session_id,
                target.owner_epoch,
                now_ms,
            )?;
            resumed += 1;
        }
        Ok(resumed)
    }

    fn pause_exhausted_worker(
        &self,
        request: WorkerInterruptionRequest,
        tab_id: &str,
    ) -> Result<bool, crate::service::plan::PlanError> {
        // Park the live worker: no close_tab, owner preserved, task re-dispatchable.
        if let Err(error) = self.plan_store.begin_provider_exhaustion_pause(
            &request.plan_id,
            &request.task_id,
            &request.attempt_id,
            &request.tab_id,
            &request.session_id,
            request.owner_epoch,
            request.occurred_at,
        ) {
            pending_interruptions()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(tab_id);
            log::error!("[worker-interruption] exhaustion pause failed: {error}");
            return Err(error);
        }
        let svc = self.clone();
        let pending_tab_id = tab_id.to_string();
        crate::async_runtime::spawn(async move {
            if let Err(error) = svc.quiesce_worker_attempt(request).await {
                log::error!("[worker-interruption] exhaustion checkpoint incident: {error}");
                svc.ui.emit_native(
                    "coordination-incident",
                    serde_json::json!({
                        "kind": "provider_exhaustion_checkpoint_failed",
                        "impact": error,
                        "fallback": "Worker is paused and preserved; it resumes at quota reset."
                    }),
                );
            }
            pending_interruptions()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&pending_tab_id);
        });
        Ok(true)
    }

    pub async fn quiesce_worker_attempt(
        &self,
        request: WorkerInterruptionRequest,
    ) -> Result<(), String> {
        let checkpoint_request = request.clone();
        let checkpoint = tokio::task::spawn_blocking(move || {
            seal_git_checkpoint(CheckpointRequest {
                task_id: &checkpoint_request.task_id,
                attempt_id: &checkpoint_request.attempt_id,
                tab_id: &checkpoint_request.tab_id,
                session_id: &checkpoint_request.session_id,
                owner_epoch: checkpoint_request.owner_epoch,
                repo_path: &checkpoint_request.repo_path,
                worktree_path: &checkpoint_request.worktree_path,
                base_head: &checkpoint_request.base_head,
                completed_tests: checkpoint_request.completed_tests,
                unresolved_work: checkpoint_request.unresolved_work,
                next_acceptance_gate: checkpoint_request.next_acceptance_gate,
            })
        })
        .await
        .map_err(|error| format!("checkpoint worker failed: {error}"))??;
        self.plan_store
            .finish_provider_exhaustion_pause(
                &request.plan_id,
                &request.task_id,
                &request.attempt_id,
                request.owner_epoch,
                checkpoint,
                request.occurred_at,
            )
            .map_err(|error| error.to_string())
    }

    pub fn signal_worker_interruption(
        &self,
        tab_id: &str,
        event: WorkerInterruptionEvent,
    ) -> Result<bool, crate::service::plan::PlanError> {
        if event != WorkerInterruptionEvent::ProviderExhausted
            && self.restart_carries_worker_interruption(tab_id)
        {
            return Ok(false);
        }
        if pending_interruptions()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(tab_id)
        {
            return Ok(true);
        }
        let Some(owned) = self.plan_store.task_interruption_projection(tab_id)? else {
            return Ok(false);
        };
        let request = WorkerInterruptionRequest {
            plan_id: owned.plan_id,
            task_id: owned.task_id,
            attempt_id: owned.attempt_id,
            tab_id: tab_id.to_string(),
            session_id: owned.owner_session_id,
            owner_epoch: owned.owner_epoch,
            repo_path: PathBuf::from(owned.repo_path),
            worktree_path: PathBuf::from(owned.worktree_path),
            base_head: owned.base_head.unwrap_or_else(|| "HEAD".into()),
            completed_tests: vec![],
            unresolved_work: "Worker interrupted before an accepted report".into(),
            next_acceptance_gate: owned
                .test_plan
                .unwrap_or_else(|| "Review the sealed checkpoint before continuing".into()),
            event,
            occurred_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
        let claimed = pending_interruptions()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(tab_id.to_string());
        if !claimed {
            return Ok(true);
        }
        // Provider exhaustion PRESERVES the live worker: pause/quiesce it in a re-dispatchable state and NEVER close its tab.
        if request.event == WorkerInterruptionEvent::ProviderExhausted {
            return self.pause_exhausted_worker(request, tab_id);
        }
        if let Err(error) = self.plan_store.begin_task_interruption(
            &request.plan_id,
            &request.task_id,
            &request.attempt_id,
            &request.tab_id,
            &request.session_id,
            request.owner_epoch,
            request.event.reason(),
            request.occurred_at,
        ) {
            pending_interruptions()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(tab_id);
            log::error!("[worker-interruption] fence failed: {error}");
            return Err(error);
        }
        let loss_read_error = if request.event == WorkerInterruptionEvent::ProcessExited {
            match self.worker_loss_for_task(
                &request.plan_id,
                &request.task_id,
                tab_id,
                crate::service::worker_loss::WorkerLossCause::ProcessExited,
            ) {
                Ok(loss) => {
                    self.report_worker_losses(loss.into_iter().collect());
                    None
                }
                Err(error) => {
                    log::error!("[worker-lost] fenced interruption recovery read failed: {error}");
                    Some(error)
                }
            }
        } else {
            None
        };
        let svc = self.clone();
        let pending_tab_id = tab_id.to_string();
        crate::async_runtime::spawn(async move {
            if let Err(error) = svc.interrupt_worker_attempt(request).await {
                log::error!("[worker-interruption] coordination incident: {error}");
                svc.ui.emit_native(
                    "coordination-incident",
                    serde_json::json!({
                        "kind": "worker_interruption_checkpoint_failed",
                        "impact": error,
                        "fallback": "Preserve the worker lane and inspect it before reassignment."
                    }),
                );
            }
            pending_interruptions()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&pending_tab_id);
        });
        match loss_read_error {
            Some(error) => Err(error),
            None => Ok(true),
        }
    }

    pub async fn interrupt_worker_attempt(
        &self,
        request: WorkerInterruptionRequest,
    ) -> Result<(), String> {
        let checkpoint_request = request.clone();
        let checkpoint = tokio::task::spawn_blocking(move || {
            seal_git_checkpoint(CheckpointRequest {
                task_id: &checkpoint_request.task_id,
                attempt_id: &checkpoint_request.attempt_id,
                tab_id: &checkpoint_request.tab_id,
                session_id: &checkpoint_request.session_id,
                owner_epoch: checkpoint_request.owner_epoch,
                repo_path: &checkpoint_request.repo_path,
                worktree_path: &checkpoint_request.worktree_path,
                base_head: &checkpoint_request.base_head,
                completed_tests: checkpoint_request.completed_tests,
                unresolved_work: checkpoint_request.unresolved_work,
                next_acceptance_gate: checkpoint_request.next_acceptance_gate,
            })
        })
        .await
        .map_err(|error| {
            let _ = self.plan_store.mark_interruption_checkpoint_failed(
                &request.plan_id,
                &request.task_id,
                &request.attempt_id,
                request.owner_epoch,
                request.occurred_at,
            );
            format!("checkpoint worker failed: {error}")
        })?
        .inspect_err(|_error| {
            let _ = self.plan_store.mark_interruption_checkpoint_failed(
                &request.plan_id,
                &request.task_id,
                &request.attempt_id,
                request.owner_epoch,
                request.occurred_at,
            );
        })?;
        self.plan_store
            .finish_task_interruption(
                &request.plan_id,
                &request.task_id,
                &request.attempt_id,
                request.owner_epoch,
                checkpoint,
                request.occurred_at,
            )
            .map_err(|error| error.to_string())
    }

    pub async fn adopt_worker_checkpoint(
        &self,
        task_id: &str,
        new_tab_id: &str,
        new_session_id: &str,
        now_ms: u64,
    ) -> Result<(u64, String), String> {
        let service = self.clone();
        let task_id = task_id.to_string();
        let new_tab_id = new_tab_id.to_string();
        let new_session_id = new_session_id.to_string();
        tokio::task::spawn_blocking(move || {
            service.adopt_worker_checkpoint_sync(&task_id, &new_tab_id, &new_session_id, now_ms)
        })
        .await
        .map_err(|error| format!("checkpoint adoption worker failed: {error}"))?
    }

    pub fn adopt_worker_checkpoint_sync(
        &self,
        task_id: &str,
        new_tab_id: &str,
        new_session_id: &str,
        now_ms: u64,
    ) -> Result<(u64, String), String> {
        self.authenticate_checkpoint_adopter(new_tab_id, new_session_id)?;
        let detail = self
            .plan_store
            .task_checkpoint_adoption_projection(task_id)
            .map_err(|error| error.to_string())?;
        let checkpoint = detail
            .checkpoint
            .ok_or_else(|| "task has no interruption checkpoint".to_string())?;
        verify_git_checkpoint(&checkpoint)?;
        self.persist_checkpoint_adoption(
            &detail.plan_id,
            task_id,
            new_tab_id,
            new_session_id,
            checkpoint,
            now_ms,
        )
    }

    fn authenticate_checkpoint_adopter(
        &self,
        new_tab_id: &str,
        new_session_id: &str,
    ) -> Result<(), String> {
        let authenticated = self.mux.read().get_tab(new_tab_id).is_some_and(|tab| {
            tab.agent_session_id.as_deref() == Some(new_session_id)
                && tab.session_binding_nonce.as_ref().is_some_and(|nonce| {
                    tab.reported_session_binding_nonce.as_deref() == Some(nonce.as_str())
                })
        });
        if !authenticated {
            return Err("adoption requires authenticated fresh-owner session binding".into());
        }
        Ok(())
    }

    fn persist_checkpoint_adoption(
        &self,
        plan_id: &str,
        task_id: &str,
        new_tab_id: &str,
        new_session_id: &str,
        checkpoint: InterruptionCheckpoint,
        now_ms: u64,
    ) -> Result<(u64, String), String> {
        let (captured, binding, prior) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(new_tab_id).ok_or("adoption tab disappeared")?;
            let binding = crate::service::task_assignment::authenticated_tab_binding(tab)
                .filter(|binding| binding.session_id == new_session_id)
                .ok_or("adoption session binding changed")?;
            let prior = tab.task_binding.clone().unwrap_or_default();
            if prior.plan_id.as_deref().is_some_and(|id| id != plan_id)
                || prior.task_id.as_deref().is_some_and(|id| id != task_id)
                || prior
                    .attempt_id
                    .as_deref()
                    .is_some_and(|id| id != checkpoint.attempt_id)
            {
                return Err("adoption tab already owns a different task attempt".into());
            }
            (tab.clone(), binding, prior)
        };
        let receipt = self
            .plan_store
            .adopt_interrupted_task_with_undo(
                plan_id,
                task_id,
                &checkpoint.diff_sha256,
                &checkpoint.base_head,
                new_tab_id,
                &binding,
                now_ms,
            )
            .map_err(|error| error.to_string())?;
        let adopted = receipt.outcome();
        let committed = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(new_tab_id)
                .filter(|tab| {
                    tab.agent_type == captured.agent_type
                        && tab.agent_session_id == captured.agent_session_id
                        && tab.session_binding_nonce == captured.session_binding_nonce
                        && tab.reported_session_binding_nonce
                            == captured.reported_session_binding_nonce
                        && tab.task_binding == captured.task_binding
                        && tab.role == captured.role
                        && tab.role_profile == captured.role_profile
                        && tab.team_id == captured.team_id
                        && tab.parent_tab_id == captured.parent_tab_id
                        && tab.lifecycle_epoch == captured.lifecycle_epoch
                })
                .map(|tab| {
                    tab.task_binding = Some(crate::multiplexer::types::TaskBinding {
                        plan_id: Some(plan_id.into()),
                        task_id: Some(task_id.into()),
                        attempt_id: Some(adopted.1.clone()),
                        spawn_handle: prior.spawn_handle,
                        bound_at: now_ms,
                    });
                })
                .is_some()
        };
        if !committed {
            self.plan_store
                .undo_checkpoint_adoption(receipt, now_ms)
                .map_err(|error| format!("checkpoint adoption authority conflict; {error}"))?;
            return Err(
                "checkpoint adoption conflict: live launch or assignment changed; retry by intent"
                    .into(),
            );
        }
        self.persist_current_attempt_session_model(new_tab_id);
        Ok(adopted)
    }
}

fn git(worktree: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command.args(args).current_dir(worktree);
    crate::process_util::no_window_std(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

pub fn seal_git_checkpoint(
    request: CheckpointRequest<'_>,
) -> Result<InterruptionCheckpoint, String> {
    let worktree = request
        .worktree_path
        .canonicalize()
        .map_err(|error| format!("resolve worktree: {error}"))?;
    let repo = request
        .repo_path
        .canonicalize()
        .map_err(|error| format!("resolve repository: {error}"))?;
    let current_head = git(&worktree, &["rev-parse", "HEAD"])?.trim().to_string();
    let base_head = git(&worktree, &["rev-parse", request.base_head])?
        .trim()
        .to_string();
    let git_status = git(
        &worktree,
        &["status", "--porcelain=v2", "--untracked-files=all"],
    )?;
    let tracked = git(&worktree, &["diff", "--binary", "--no-ext-diff", "HEAD"])?;
    if tracked.len() > CHECKPOINT_ARTIFACT_MAX_BYTES {
        return Err("checkpoint diff artifact exceeds 4 MiB bound".into());
    }
    let untracked = git(
        &worktree,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    let mut diff_artifact = tracked;
    if !untracked.is_empty() {
        diff_artifact.push_str("\n# CODETERM_UNTRACKED\n");
        for relative in untracked.split('\0').filter(|path| !path.is_empty()) {
            let path = worktree.join(relative);
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|error| format!("inspect untracked {relative}: {error}"))?;
            let (kind, bytes) = if metadata.file_type().is_symlink() {
                (
                    "symlink",
                    std::fs::read_link(&path)
                        .map_err(|error| format!("read untracked link {relative}: {error}"))?
                        .to_string_lossy()
                        .as_bytes()
                        .to_vec(),
                )
            } else {
                if metadata.len() > CHECKPOINT_UNTRACKED_FILE_READ_LIMIT {
                    return Err("checkpoint diff artifact exceeds 4 MiB bound".into());
                }
                let mut bytes = Vec::with_capacity(metadata.len() as usize);
                std::fs::File::open(&path)
                    .and_then(|file| {
                        use std::io::Read;
                        file.take(CHECKPOINT_UNTRACKED_FILE_READ_LIMIT)
                            .read_to_end(&mut bytes)
                    })
                    .map_err(|error| format!("read untracked {relative}: {error}"))?;
                ("file", bytes)
            };
            if diff_artifact
                .len()
                .saturating_add(relative.len())
                .saturating_add(kind.len())
                .saturating_add(bytes.len() * 2)
                .saturating_add(3)
                > CHECKPOINT_ARTIFACT_MAX_BYTES
            {
                return Err("checkpoint diff artifact exceeds 4 MiB bound".into());
            }
            diff_artifact.push_str(relative);
            diff_artifact.push('\0');
            diff_artifact.push_str(kind);
            diff_artifact.push('\0');
            for byte in bytes {
                use std::fmt::Write;
                write!(diff_artifact, "{byte:02x}").expect("string formatting cannot fail");
            }
            diff_artifact.push('\n');
        }
    }
    let diff_sha256 = checkpoint_digest(&diff_artifact);
    let checkpoint = InterruptionCheckpoint {
        task_id: request.task_id.to_string(),
        attempt_id: request.attempt_id.to_string(),
        tab_id: request.tab_id.to_string(),
        session_id: request.session_id.to_string(),
        owner_epoch: request.owner_epoch,
        repo_path: path_text(repo),
        worktree_path: path_text(worktree),
        base_head,
        current_head,
        git_status,
        diff_artifact,
        diff_sha256,
        completed_tests: request.completed_tests,
        unresolved_work: request.unresolved_work,
        next_acceptance_gate: request.next_acceptance_gate,
    };
    let encoded_len = serde_json::to_vec(&checkpoint)
        .map_err(|error| format!("encode checkpoint: {error}"))?
        .len();
    if encoded_len > CHECKPOINT_ARTIFACT_MAX_BYTES {
        return Err("checkpoint exceeds 4 MiB total bound".into());
    }
    Ok(checkpoint)
}

fn path_text(path: PathBuf) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn verify_git_checkpoint(checkpoint: &InterruptionCheckpoint) -> Result<(), String> {
    if checkpoint.diff_sha256 != checkpoint_digest(&checkpoint.diff_artifact) {
        return Err("checkpoint diff digest changed".into());
    }
    let current = seal_git_checkpoint(CheckpointRequest {
        task_id: &checkpoint.task_id,
        attempt_id: &checkpoint.attempt_id,
        tab_id: &checkpoint.tab_id,
        session_id: &checkpoint.session_id,
        owner_epoch: checkpoint.owner_epoch,
        repo_path: Path::new(&checkpoint.repo_path),
        worktree_path: Path::new(&checkpoint.worktree_path),
        base_head: &checkpoint.base_head,
        completed_tests: checkpoint.completed_tests.clone(),
        unresolved_work: checkpoint.unresolved_work.clone(),
        next_acceptance_gate: checkpoint.next_acceptance_gate.clone(),
    })?;
    if current.current_head != checkpoint.current_head
        || current.git_status != checkpoint.git_status
        || current.diff_sha256 != checkpoint.diff_sha256
    {
        return Err("checkpoint working tree changed".into());
    }
    Ok(())
}

#[cfg(any(test, feature = "test-support"))]
impl super::ServiceContext {
    pub fn persist_checkpoint_adoption_for_test(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        session: &str,
        checkpoint: InterruptionCheckpoint,
        now_ms: u64,
    ) -> Result<(u64, String), String> {
        self.persist_checkpoint_adoption(plan_id, task_id, tab_id, session, checkpoint, now_ms)
    }
}
