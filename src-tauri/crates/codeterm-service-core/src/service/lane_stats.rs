mod checkpoint;

use crate::plan::{LaneCheckpointRefusal, LaneTurnChangeStats, Plan, TaskExecutionAttempt};
use crate::service::{
    plan::{PlanError, PlanStore},
    ServiceContext,
};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use wait_timeout::ChildExt;

const TURN_CAPTURE_DEADLINE: Duration = Duration::from_secs(12);
const GIT_COMMAND_DEADLINE: Duration = Duration::from_secs(4);
const CHECKPOINTS_PER_ATTEMPT: usize = 20;
const MAX_GIT_OUTPUT_BYTES: usize = 1024 * 1024;

struct LaneAttemptTarget {
    plan_id: String,
    task_id: String,
    attempt_id: String,
    worktree_path: String,
    canonical_path: String,
    turn: u32,
}

#[derive(Debug)]
enum CaptureFailure {
    Deadline,
    Git(String),
}

#[derive(Debug)]
enum CheckpointResult {
    Created(String),
    Refused(LaneCheckpointRefusal),
}

impl ServiceContext {
    pub(crate) fn restore_lane_watches(&self) {
        for plan in self.plan_store.list_full() {
            let canonical_path = if plan.canonical_project_path.trim().is_empty() {
                &plan.project_path
            } else {
                &plan.canonical_project_path
            };
            for task in &plan.tasks {
                let lane = &task.lane;
                let Some(worktree_path) = lane.worktree_path.as_deref() else {
                    continue;
                };
                if lane.branch.is_none()
                    || lane.destination.is_none()
                    || lane.cleanup_status.as_deref().is_some_and(|status| {
                        status.eq_ignore_ascii_case("offered")
                            || status.eq_ignore_ascii_case("done")
                    })
                {
                    continue;
                }
                if let Err(error) = self
                    .repo_watch
                    .retain_lane_watch(worktree_path, Path::new(canonical_path))
                {
                    log::warn!("[lane-cleanup-offer] repository watch unavailable: {error}");
                }
            }
        }
    }

    pub(crate) fn record_lane_turn_boundary(&self, tab_id: &str) {
        let service = self.clone();
        let store = self.plan_store.clone();
        let repo_watch = self.repo_watch.clone();
        let app = self.app_handle.clone();
        let tab_id = tab_id.to_string();
        tauri::async_runtime::spawn(async move {
            let capture = tokio::task::spawn_blocking(move || {
                record_lane_turn(&service, &store, &repo_watch, &app, &tab_id)
            });
            match tokio::time::timeout(TURN_CAPTURE_DEADLINE, capture).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => log::warn!("[lane-turn-stats] capture worker failed: {error}"),
                Err(_) => log::warn!("[lane-turn-stats] capture exceeded its deadline"),
            }
        });
    }

    pub(crate) fn offer_merged_lane_cleanup(&self, repository_identity: String) {
        let store = self.plan_store.clone();
        let app = self.app_handle.clone();
        tauri::async_runtime::spawn_blocking(move || {
            offer_merged_lanes(&store, &app, &repository_identity)
        });
    }
}

fn record_lane_turn(
    service: &ServiceContext,
    store: &PlanStore,
    repo_watch: &crate::repowatch::RepoWatchRegistryHandle,
    app: &crate::app_runtime::AppHandle,
    tab_id: &str,
) {
    let Some(target) = find_lane_attempt(store, tab_id) else {
        return;
    };
    let captured_at = unix_time_millis();
    let transcript_turn_key = settled_transcript_turn_key(service, tab_id);
    let Some(turn) = reserve_turn(store, &target, captured_at, transcript_turn_key.as_deref())
    else {
        return;
    };
    let target = LaneAttemptTarget { turn, ..target };
    if let Err(error) =
        repo_watch.retain_lane_watch(&target.worktree_path, Path::new(&target.canonical_path))
    {
        log::warn!("[lane-turn-stats] repository watch unavailable: {error}");
    }

    let deadline = Instant::now() + TURN_CAPTURE_DEADLINE;
    let mut stats = LaneTurnChangeStats {
        turn: target.turn,
        captured_at,
        transcript_turn_key,
        ..Default::default()
    };
    match capture_change_stats(Path::new(&target.worktree_path), deadline) {
        Ok((numstat, staged_numstat, untracked_file_count)) => {
            stats.numstat = Some(numstat);
            stats.staged_numstat = Some(staged_numstat);
            stats.untracked_file_count = Some(untracked_file_count);
        }
        Err(CaptureFailure::Deadline) => {
            stats.checkpoint_refusal = Some(LaneCheckpointRefusal::Deadline);
        }
        Err(CaptureFailure::Git(error)) => {
            log::warn!("[lane-turn-stats] git diff failed: {error}");
            stats.checkpoint_refusal = Some(LaneCheckpointRefusal::GitFailure);
        }
    }
    if stats.numstat.is_some() {
        match create_checkpoint(
            Path::new(&target.worktree_path),
            &target.attempt_id,
            target.turn,
            deadline,
        ) {
            Ok(CheckpointResult::Created(reference)) => stats.checkpoint_ref = Some(reference),
            Ok(CheckpointResult::Refused(reason)) => stats.checkpoint_refusal = Some(reason),
            Err(CaptureFailure::Deadline) => {
                stats.checkpoint_refusal = Some(LaneCheckpointRefusal::Deadline)
            }
            Err(CaptureFailure::Git(error)) => {
                log::warn!("[lane-checkpoint] snapshot failed: {error}");
                stats.checkpoint_refusal = Some(LaneCheckpointRefusal::GitFailure);
            }
        }
    }
    if mutate_attempt(store, &target, |attempt| {
        if let Some(record) = attempt
            .lane_turn_stats
            .iter_mut()
            .find(|record| record.turn == target.turn)
        {
            *record = stats;
        }
    }) {
        codeterm_events::emit_everywhere(
            app,
            "plan-changed",
            &serde_json::json!({ "plan_id": target.plan_id }),
        );
    }
}

fn find_lane_attempt(store: &PlanStore, tab_id: &str) -> Option<LaneAttemptTarget> {
    store.list_full().into_iter().find_map(|plan| {
        plan.tasks.iter().find_map(|task| {
            let attempt = task.execution_attempts.iter().rev().find(|attempt| {
                attempt.executor_tab_id == tab_id
                    && attempt.closed_at.is_none()
                    && attempt.placement.is_isolated_lane()
                    && attempt.worktree_path.is_some()
            })?;
            let worktree_path = attempt.worktree_path.clone()?;
            let canonical_path = if plan.canonical_project_path.trim().is_empty() {
                plan.project_path.clone()
            } else {
                plan.canonical_project_path.clone()
            };
            Some(LaneAttemptTarget {
                plan_id: plan.id.clone(),
                task_id: task.id.clone(),
                attempt_id: attempt.id.clone(),
                worktree_path,
                canonical_path,
                turn: attempt
                    .lane_turn_stats
                    .iter()
                    .map(|stats| stats.turn)
                    .max()
                    .unwrap_or_default(),
            })
        })
    })
}

fn reserve_turn(
    store: &PlanStore,
    target: &LaneAttemptTarget,
    captured_at: u64,
    transcript_turn_key: Option<&str>,
) -> Option<u32> {
    store
        .with_plan(&target.plan_id, captured_at, |plan| {
            let attempt = find_attempt_mut(plan, &target.task_id, &target.attempt_id)
                .ok_or(PlanError::TaskNotFound)?;
            if is_duplicate_transcript_turn(&attempt.lane_turn_stats, transcript_turn_key) {
                return Ok(None);
            }
            let turn = attempt
                .lane_turn_stats
                .iter()
                .map(|stats| stats.turn)
                .max()
                .unwrap_or_default()
                .saturating_add(1);
            attempt.lane_turn_stats.push(LaneTurnChangeStats {
                turn,
                captured_at,
                transcript_turn_key: transcript_turn_key.map(str::to_string),
                ..Default::default()
            });
            Ok(Some(turn))
        })
        .ok()
        .flatten()
}

fn settled_transcript_turn_key(service: &ServiceContext, tab_id: &str) -> Option<String> {
    let (provider, session_id) = {
        let mux = service.mux.read();
        let tab = mux.get_tab(tab_id)?;
        (tab.agent_type.clone()?, tab.agent_session_id.clone()?)
    };
    let (messages, _) = crate::service::tab_chat::tab_chat_snapshot(service, tab_id, 0).ok()?;
    transcript_turn_key(&provider, &session_id, &messages)
}

fn transcript_turn_key(
    provider: &str,
    session_id: &str,
    messages: &[crate::agent::chat_types::ChatMsg],
) -> Option<String> {
    use sha2::Digest;

    let message = messages
        .iter()
        .rev()
        .find(|message| message.kind.eq_ignore_ascii_case("assistant"))?;
    if message.id.trim().is_empty() && message.seq == 0 {
        return None;
    }
    let identity = format!("{provider}\0{session_id}\0{}\0{}", message.id, message.seq);
    Some(format!("{:x}", sha2::Sha256::digest(identity.as_bytes())))
}

fn is_duplicate_transcript_turn(
    stats: &[LaneTurnChangeStats],
    transcript_turn_key: Option<&str>,
) -> bool {
    transcript_turn_key.is_some_and(|key| {
        stats
            .iter()
            .any(|stats| stats.transcript_turn_key.as_deref() == Some(key))
    })
}

fn mutate_attempt(
    store: &PlanStore,
    target: &LaneAttemptTarget,
    update: impl FnOnce(&mut TaskExecutionAttempt),
) -> bool {
    store
        .with_plan(&target.plan_id, unix_time_millis(), |plan| {
            let attempt = find_attempt_mut(plan, &target.task_id, &target.attempt_id)
                .ok_or(PlanError::TaskNotFound)?;
            update(attempt);
            Ok(())
        })
        .is_ok()
}

fn find_attempt_mut<'a>(
    plan: &'a mut Plan,
    task_id: &str,
    attempt_id: &str,
) -> Option<&'a mut TaskExecutionAttempt> {
    plan.tasks
        .iter_mut()
        .find(|task| task.id == task_id)?
        .execution_attempts
        .iter_mut()
        .find(|attempt| attempt.id == attempt_id)
}

fn capture_change_stats(
    cwd: &Path,
    deadline: Instant,
) -> Result<(String, String, u32), CaptureFailure> {
    let unstaged = run_git(cwd, &["diff-files", "--numstat"], None, deadline)?;
    let staged = run_git(
        cwd,
        &["diff-index", "--cached", "-M", "--numstat", "HEAD"],
        None,
        deadline,
    )?;
    let untracked = run_git(
        cwd,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        None,
        deadline,
    )?;
    let count = untracked
        .as_bytes()
        .iter()
        .filter(|byte| **byte == 0)
        .count();
    Ok((unstaged, staged, count.min(u32::MAX as usize) as u32))
}

fn create_checkpoint(
    cwd: &Path,
    attempt_id: &str,
    turn: u32,
    deadline: Instant,
) -> Result<CheckpointResult, CaptureFailure> {
    checkpoint::create(
        &mut GitCheckpoint {
            cwd,
            deadline,
            index: None,
        },
        attempt_id,
        turn,
    )
}

struct GitCheckpoint<'a> {
    cwd: &'a Path,
    deadline: Instant,
    index: Option<tempfile::TempDir>,
}

impl checkpoint::Repository for GitCheckpoint<'_> {
    fn uses_lfs(&mut self) -> Result<bool, CaptureFailure> {
        repository_uses_lfs(self.cwd, self.deadline)
    }

    fn non_cone_sparse(&mut self) -> Result<bool, CaptureFailure> {
        has_non_cone_sparse_checkout(self.cwd, self.deadline)
    }

    fn run(&mut self, args: &[&str], isolated_index: bool) -> Result<String, CaptureFailure> {
        let index = if isolated_index {
            if self.index.is_none() {
                self.index = Some(
                    tempfile::Builder::new()
                        .prefix("codeterm-lane-checkpoint-")
                        .tempdir()
                        .map_err(|error| CaptureFailure::Git(error.to_string()))?,
                );
            }
            self.index
                .as_ref()
                .map(|directory| directory.path().join("index"))
        } else {
            None
        };
        run_git(
            self.cwd,
            args,
            index
                .as_ref()
                .map(|path| ("GIT_INDEX_FILE", path.as_os_str())),
            self.deadline,
        )
    }
}

fn repository_uses_lfs(cwd: &Path, deadline: Instant) -> Result<bool, CaptureFailure> {
    let attributes = run_git(
        cwd,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            ".gitattributes",
            ":(glob)**/.gitattributes",
        ],
        None,
        deadline,
    )?;
    for raw_path in attributes.split('\0').filter(|path| !path.is_empty()) {
        let path = cwd.join(raw_path);
        let Ok(contents) = std::fs::read_to_string(path) else {
            continue;
        };
        if contents.lines().any(attribute_uses_lfs) {
            return Ok(true);
        }
    }
    let global_attributes = run_git(
        cwd,
        &["config", "--path", "core.attributesFile"],
        None,
        deadline,
    )
    .ok()
    .map(|path| PathBuf::from(path.trim()))
    .filter(|path| path.is_file());
    if global_attributes
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|contents| contents.lines().any(attribute_uses_lfs))
    {
        return Ok(true);
    }
    match run_git_status(cwd, &["lfs", "ls-files", "--name-only"], deadline) {
        Ok(output) => Ok(output.status.success() && !output.stdout.is_empty()),
        Err(CaptureFailure::Deadline) => Err(CaptureFailure::Deadline),
        Err(CaptureFailure::Git(_)) => Ok(false),
    }
}

fn attribute_uses_lfs(line: &str) -> bool {
    line.split_whitespace()
        .any(|token| token.trim_matches('"').eq_ignore_ascii_case("filter=lfs"))
}

fn has_non_cone_sparse_checkout(cwd: &Path, deadline: Instant) -> Result<bool, CaptureFailure> {
    let sparse = run_git_status(cwd, &["config", "--bool", "core.sparseCheckout"], deadline)?;
    if !sparse.status.success()
        || !String::from_utf8_lossy(&sparse.stdout)
            .trim()
            .eq_ignore_ascii_case("true")
    {
        return Ok(false);
    }
    let cone = run_git_status(
        cwd,
        &["config", "--bool", "core.sparseCheckoutCone"],
        deadline,
    )?;
    Ok(!cone.status.success()
        || !String::from_utf8_lossy(&cone.stdout)
            .trim()
            .eq_ignore_ascii_case("true"))
}

pub(crate) fn delete_attempt_checkpoint_refs(cwd: &Path, attempt_id: &str) -> Result<(), String> {
    if !safe_ref_component(attempt_id) {
        return Ok(());
    }
    let deadline = Instant::now() + TURN_CAPTURE_DEADLINE;
    let prefix = format!("refs/domios/checkpoints/{attempt_id}");
    let refs = run_git(
        cwd,
        &["for-each-ref", "--format=%(refname)", &prefix],
        None,
        deadline,
    )
    .map_err(capture_failure_text)?;
    for reference in refs
        .lines()
        .filter(|reference| checkpoint_turn(&prefix, reference).is_some())
    {
        run_git(cwd, &["update-ref", "-d", reference], None, deadline)
            .map_err(capture_failure_text)?;
    }
    Ok(())
}

fn checkpoint_turn(prefix: &str, reference: &str) -> Option<u32> {
    reference
        .strip_prefix(prefix)?
        .strip_prefix('/')?
        .parse()
        .ok()
}

fn safe_ref_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn run_git(
    cwd: &Path,
    args: &[&str],
    index: Option<(&str, &std::ffi::OsStr)>,
    deadline: Instant,
) -> Result<String, CaptureFailure> {
    let output = run_git_status_with_index(cwd, args, index, deadline)?;
    if !output.status.success() {
        return Err(CaptureFailure::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_git_status(cwd: &Path, args: &[&str], deadline: Instant) -> Result<Output, CaptureFailure> {
    run_git_status_with_index(cwd, args, None, deadline)
}

fn run_git_status_with_index(
    cwd: &Path,
    args: &[&str],
    index: Option<(&str, &std::ffi::OsStr)>,
    deadline: Instant,
) -> Result<Output, CaptureFailure> {
    let mut command = Command::new("git");
    crate::process_util::no_window_std(&mut command);
    // Optional locks off: these reads run beside the agent's own git and must not take index.lock.
    command
        .current_dir(cwd)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some((key, value)) = index {
        command.env(key, value);
    }
    let mut child = command
        .spawn()
        .map_err(|error| CaptureFailure::Git(error.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CaptureFailure::Git("git stdout pipe missing".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CaptureFailure::Git("git stderr pipe missing".into()))?;
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .min(GIT_COMMAND_DEADLINE);
    let command_deadline = Instant::now() + remaining;
    let (sender, receiver) = std::sync::mpsc::channel();
    let stdout_sender = sender.clone();
    let stdout_reader = thread::spawn(move || {
        let _ = stdout_sender.send((true, read_bounded(stdout)));
    });
    let stderr_reader = thread::spawn(move || {
        let _ = sender.send((false, read_bounded(stderr)));
    });
    let waited = if remaining.is_zero() {
        None
    } else {
        match child.wait_timeout(remaining) {
            Ok(status) => status,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CaptureFailure::Git(error.to_string()));
            }
        }
    };
    let Some(status) = waited else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(CaptureFailure::Deadline);
    };
    let (mut stdout, mut stderr) = (None, None);
    while stdout.is_none() || stderr.is_none() {
        let timeout = command_deadline.saturating_duration_since(Instant::now());
        let (is_stdout, output) = receiver
            .recv_timeout(timeout)
            .map_err(|_| CaptureFailure::Deadline)?;
        let (bytes, exceeded_limit) =
            output.map_err(|error| CaptureFailure::Git(error.to_string()))?;
        if exceeded_limit {
            return Err(CaptureFailure::Git(
                "git command output exceeded its one MiB bound".into(),
            ));
        }
        if is_stdout {
            stdout = Some(bytes);
        } else {
            stderr = Some(bytes);
        }
    }
    let stdout = stdout.ok_or_else(|| CaptureFailure::Git("git stdout capture missing".into()))?;
    let stderr = stderr.ok_or_else(|| CaptureFailure::Git("git stderr capture missing".into()))?;
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn read_bounded(mut reader: impl std::io::Read) -> std::io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut exceeded_limit = false;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let available = MAX_GIT_OUTPUT_BYTES.saturating_sub(output.len());
        let retained = read.min(available);
        output.extend_from_slice(&buffer[..retained]);
        exceeded_limit |= retained < read;
    }
    Ok((output, exceeded_limit))
}

fn capture_failure_text(error: CaptureFailure) -> String {
    match error {
        CaptureFailure::Deadline => "git checkpoint operation exceeded its deadline".into(),
        CaptureFailure::Git(error) => error,
    }
}

fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn offer_merged_lanes(
    store: &PlanStore,
    app: &crate::app_runtime::AppHandle,
    repository_identity: &str,
) {
    let deadline = Instant::now() + TURN_CAPTURE_DEADLINE;
    let mut checked = 0usize;
    for plan in store.list_full() {
        let canonical_path = if plan.canonical_project_path.trim().is_empty() {
            &plan.project_path
        } else {
            &plan.canonical_project_path
        };
        if !super::worker_lane::same_path(canonical_path, repository_identity) {
            continue;
        }
        for task in &plan.tasks {
            if checked >= 32 {
                return;
            }
            let lane = &task.lane;
            let Some(branch) = lane.branch.as_deref().filter(|value| !value.is_empty()) else {
                continue;
            };
            let Some(destination) = lane
                .destination
                .as_deref()
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            if lane.cleanup_status.as_deref().is_some_and(|status| {
                status.eq_ignore_ascii_case("offered") || status.eq_ignore_ascii_case("done")
            }) {
                continue;
            }
            checked += 1;
            match branch_is_ancestor(Path::new(canonical_path), branch, destination, deadline) {
                Ok(true) => {
                    let worktree = lane.worktree_path.as_deref().unwrap_or_default();
                    let command = super::lane_removal::lane_remove_command(
                        worktree,
                        Some(branch),
                        canonical_path,
                    );
                    let update = crate::service::plan::TaskLaneUpdate {
                        cleanup_status: Some(Some("offered".into())),
                        cleanup_hints: Some(vec![command.clone()]),
                        ..Default::default()
                    };
                    if store
                        .update_task_lane(&plan.id, &task.id, update, unix_time_millis())
                        .is_ok()
                    {
                        codeterm_events::emit_everywhere(
                            app,
                            "plan-changed",
                            &serde_json::json!({ "plan_id": plan.id }),
                        );
                        codeterm_events::emit_everywhere(
                            app,
                            "plan-lane-cleanup-offer",
                            &serde_json::json!({
                                "plan_id": plan.id,
                                "task_id": task.id,
                                "branch": branch,
                                "destination": destination,
                                "command": command,
                            }),
                        );
                    }
                }
                Ok(false) => {}
                Err(error) => log::warn!("[lane-cleanup-offer] ancestry check failed: {error:?}"),
            }
        }
    }
}

fn branch_is_ancestor(
    cwd: &Path,
    branch: &str,
    destination: &str,
    deadline: Instant,
) -> Result<bool, CaptureFailure> {
    let output = run_git_status(
        cwd,
        &["merge-base", "--is-ancestor", branch, destination],
        deadline,
    )?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(CaptureFailure::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        )),
    }
}

#[cfg(test)]
mod tests;
