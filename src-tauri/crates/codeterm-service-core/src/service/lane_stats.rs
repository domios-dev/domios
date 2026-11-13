use codeterm_lanes::checkpoint_io::checkpoint_turn;
pub use codeterm_lanes::checkpoint_io::{
    capture_failure_text, delete_attempt_checkpoint_refs, CaptureFailure,
};
use codeterm_lanes::checkpoint_io::{run_git, run_git_status, safe_ref_component};
mod checkpoint;

use crate::plan::{LaneCheckpointRefusal, LaneTurnChangeStats};
use crate::service::{
    plan::{PlanError, PlanStore},
    ServiceContext,
};
use std::path::{Path, PathBuf};

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TURN_CAPTURE_DEADLINE: Duration = Duration::from_secs(12);
const CHECKPOINTS_PER_ATTEMPT: usize = 20;

pub use crate::service::plan::LaneAttemptRead as LaneAttemptTarget;

#[derive(Debug)]
pub enum CheckpointResult {
    Created(String),
    Refused(LaneCheckpointRefusal),
}

impl ServiceContext {
    pub fn restore_lane_watches(&self) -> Result<(), PlanError> {
        for lane in self.plan_store.lane_watch_projection()? {
            if let Err(error) = self
                .ext::<crate::repowatch::RepoWatchRegistryHandle>()
                .retain_lane_watch(&lane.worktree_path, Path::new(&lane.canonical_path))
            {
                log::debug!("[lane-watch] repository watch unavailable: {error}");
            }
        }
        Ok(())
    }

    pub(crate) fn record_lane_turn_boundary(&self, tab_id: &str) {
        let service = self.clone();
        let tab_id = tab_id.to_string();
        crate::async_runtime::spawn_blocking(move || {
            let target = match find_lane_attempt(&service.plan_store, &tab_id) {
                Ok(Some(target)) => target,
                Ok(None) => return,
                Err(error) => {
                    log::error!("[lane-turn-stats] cannot read lane attempt for {tab_id}: {error}");
                    return;
                }
            };
            let repository = target.canonical_path.clone();
            let worker = service.clone();
            service.enqueue_lane_git(
                &repository,
                format!("turn:{tab_id}"),
                Box::new(move || {
                    record_lane_turn(
                        &worker,
                        &worker.plan_store,
                        worker.ext::<crate::repowatch::RepoWatchRegistryHandle>(),
                        worker.ui.as_ref(),
                        &tab_id,
                    )
                }),
            );
        });
    }
}

fn record_lane_turn(
    service: &ServiceContext,
    store: &PlanStore,
    repo_watch: &crate::repowatch::RepoWatchRegistryHandle,
    ui: &dyn crate::ports::host::UiEvents,
    tab_id: &str,
) {
    let target = match find_lane_attempt(store, tab_id) {
        Ok(Some(target)) => target,
        Ok(None) => return,
        Err(error) => {
            log::error!("[lane-turn-stats] cannot read lane attempt for {tab_id}: {error}");
            return;
        }
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
        log::debug!("[lane-turn-stats] repository watch unavailable: {error}");
    }

    let mut stats = LaneTurnChangeStats {
        turn: target.turn,
        captured_at,
        transcript_turn_key,
        ..Default::default()
    };
    capture_turn(&target, &mut stats, Instant::now() + TURN_CAPTURE_DEADLINE);
    if mutate_attempt(store, &target, |turn_stats| {
        if let Some(record) = turn_stats
            .iter_mut()
            .find(|record| record.turn == target.turn)
        {
            *record = stats;
        }
    }) {
        crate::service::plan::change_events::emit_committed_plan_change(ui, store, &target.plan_id);
    }
}

fn capture_turn(target: &LaneAttemptTarget, stats: &mut LaneTurnChangeStats, deadline: Instant) {
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
    if stats.numstat.is_some() && may_checkpoint(target) {
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
}

/// Checkpoint refs and objects are written only into a checkout Domios provisioned;
/// a foreign or unverifiable checkout keeps its stats but gets nothing written.
fn may_checkpoint(target: &LaneAttemptTarget) -> bool {
    crate::service::plan::lane_ownership::unmanaged_lane_reason_git(
        target.ownership,
        &target.canonical_path,
        Some(&target.worktree_path),
        target.branch.as_deref(),
    )
    .is_none()
}

pub(crate) fn find_lane_attempt(
    store: &PlanStore,
    tab_id: &str,
) -> Result<Option<LaneAttemptTarget>, PlanError> {
    store.lane_attempt_projection(tab_id)
}

fn reserve_turn(
    store: &PlanStore,
    target: &LaneAttemptTarget,
    captured_at: u64,
    transcript_turn_key: Option<&str>,
) -> Option<u32> {
    store
        .edit_lane_turn_stats(
            &target.plan_id,
            &target.task_id,
            &target.attempt_id,
            captured_at,
            |stats| {
                if is_duplicate_transcript_turn(stats, transcript_turn_key) {
                    return Ok(None);
                }
                let turn = stats
                    .iter()
                    .map(|stats| stats.turn)
                    .max()
                    .unwrap_or_default()
                    .saturating_add(1);
                stats.push(LaneTurnChangeStats {
                    turn,
                    captured_at,
                    transcript_turn_key: transcript_turn_key.map(str::to_string),
                    ..Default::default()
                });
                Ok(Some(turn))
            },
        )
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
    update: impl FnOnce(&mut Vec<LaneTurnChangeStats>),
) -> bool {
    store
        .edit_lane_turn_stats(
            &target.plan_id,
            &target.task_id,
            &target.attempt_id,
            unix_time_millis(),
            |stats| {
                update(stats);
                Ok(Some(()))
            },
        )
        .is_ok()
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

pub struct GitCheckpoint<'a> {
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

fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests;
